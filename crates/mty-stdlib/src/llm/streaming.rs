//! Streaming completions for `std.llm`.
//!
//! Two surfaces live here:
//!
//! - [`MessageStream`] — a typed `Stream` of
//!   [`crate::llm::message::MessageDelta`] items, returned by
//!   `LlmProvider::complete_stream`. Generic over the underlying
//!   stream so each provider can produce one without boxing.
//! - [`parse_anthropic_sse`] — a synchronous SSE-event parser used by
//!   the Anthropic client + the streaming-fixture tests. Keeping the
//!   parser pure (`&str` -> `Vec<MessageDelta>`) means tests can feed
//!   captured fixtures without spinning a server.
//!
//! ## SSE shape
//!
//! Anthropic's `messages.stream` emits a sequence of named events:
//!
//! ```text
//! event: message_start
//! data: {"type":"message_start","message":{...}}
//!
//! event: content_block_start
//! data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}
//!
//! event: content_block_delta
//! data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}
//!
//! event: content_block_delta
//! data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}
//!
//! event: message_delta
//! data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{...}}
//!
//! event: message_stop
//! data: {"type":"message_stop"}
//! ```
//!
//! We project this onto the three [`MessageDelta`] variants — text
//! deltas accumulate into `TextDelta`, tool-use deltas accumulate
//! into `ToolUseDelta` (stitching the fragmented `input_json_delta`
//! payloads), and the terminal `message_delta` becomes
//! `MessageDelta::Done`. Events we don't recognise are dropped — the
//! Anthropic team adds new event types regularly and dropping is the
//! correct forward-compatibility behaviour for a streaming parser.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;
use futures_util::StreamExt;

use crate::llm::error::LlmError;
use crate::llm::message::MessageDelta;

/// A typed stream of incremental message deltas.
///
/// Type-erased over the underlying byte stream so callers can hold
/// `MessageStream` without naming the provider-specific generic.
pub struct MessageStream {
    inner: Pin<Box<dyn Stream<Item = Result<MessageDelta, LlmError>> + Send>>,
}

impl MessageStream {
    /// Wrap any `Stream<Item = Result<MessageDelta, LlmError>>` into the
    /// typed `MessageStream`. Provider clients call this once at the
    /// end of `complete_stream`.
    pub fn new<S>(s: S) -> Self
    where
        S: Stream<Item = Result<MessageDelta, LlmError>> + Send + 'static,
    {
        Self { inner: Box::pin(s) }
    }

    /// Convenience: build a `MessageStream` from a finished `Vec` of
    /// deltas. Used by the skeleton OpenAI/Gemini/Bedrock clients to
    /// satisfy the trait without spinning a real stream.
    pub fn from_vec(deltas: Vec<Result<MessageDelta, LlmError>>) -> Self {
        Self::new(futures_util::stream::iter(deltas))
    }

    /// v0.27 Track E (QoL #2) — source-level iteration entry point.
    ///
    /// Pull the next `MessageDelta` from the stream, awaiting the
    /// underlying poll. Returns `None` once the upstream stream is
    /// exhausted, matching the standard `Iterator::next` shape so
    /// Mighty source can write either
    ///
    /// ```ignore
    /// while let Some(delta) = stream.next().await {
    ///   match delta { ... }
    /// }
    /// ```
    ///
    /// or — once the `for delta in stream` lowering is in place — the
    /// `for`-loop sugar. Errors collapse to a `Done` terminal with the
    /// error text in the stop-reason; consumers that need richer error
    /// handling should drop down to the `Stream` impl directly.
    pub async fn next(&mut self) -> Option<MessageDelta> {
        match self.inner.next().await {
            Some(Ok(d)) => Some(d),
            Some(Err(e)) => Some(MessageDelta::Done {
                stop_reason: format!("stream_error: {e}"),
            }),
            None => None,
        }
    }

    /// v0.27 Track E (QoL #2) — synchronous adapter used by the
    /// SIR interpreter's `eval_method` dispatch.
    ///
    /// Block on `next()` so a Mighty `while let Some(d) = stream.next() { ... }`
    /// loop compiles cleanly when the interpreter runs outside an
    /// `await` context. When the caller is already inside a tokio
    /// runtime (which the slice-7 driver always is), we use
    /// `tokio::task::block_in_place` + `Handle::block_on`; otherwise
    /// we spin up a minimal `current_thread` runtime just for the poll.
    pub fn next_blocking(&mut self) -> Option<MessageDelta> {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            // We're inside a tokio runtime. `block_in_place` is only
            // available on multi-thread runtimes; fall back to a
            // dedicated single-threaded runtime when it isn't.
            match handle.runtime_flavor() {
                tokio::runtime::RuntimeFlavor::MultiThread => {
                    tokio::task::block_in_place(|| handle.block_on(self.next()))
                }
                _ => fresh_runtime_block(self),
            }
        } else {
            fresh_runtime_block(self)
        }
    }
}

fn fresh_runtime_block(s: &mut MessageStream) -> Option<MessageDelta> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    rt.block_on(s.next())
}

impl Stream for MessageStream {
    type Item = Result<MessageDelta, LlmError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

/// Parse a buffer of SSE bytes into the typed delta stream.
///
/// `input` is the full text body of an Anthropic streaming response
/// (or a chunk of it — the parser is restartable as long as the
/// caller splits on `\n\n` blank-line boundaries). The return is
/// every delta that could be extracted *plus* a tail of bytes we
/// haven't yet seen the end of (caller prepends to the next chunk).
///
/// Pulled out as a free function so `tests/llm_streaming.rs` can
/// drive it against captured fixtures without async/network.
pub fn parse_anthropic_sse(input: &str) -> (Vec<MessageDelta>, String) {
    let mut deltas = Vec::new();
    // SSE events are separated by `\n\n` per the spec. Anything after
    // the last separator is an incomplete tail; hand it back so the
    // caller can re-feed it. We normalise CRLF → LF first so that
    // captured fixtures checked out on Windows (core.autocrlf=true)
    // and any upstream proxy that rewrites line endings still parse.
    let normalised: String = if input.contains("\r\n") {
        input.replace("\r\n", "\n")
    } else {
        input.to_string()
    };
    let (complete_owned, tail) = match normalised.rsplit_once("\n\n") {
        Some((c, t)) => (c.to_string(), t.to_string()),
        None => return (deltas, normalised),
    };
    let complete = complete_owned.as_str();

    // Track per-content-block accumulator state for tool_use blocks.
    // We don't need to track text blocks because they're emitted
    // delta-by-delta and the caller is responsible for joining.
    let mut current_tool_id: Option<(String, String)> = None;

    for raw_event in complete.split("\n\n") {
        let mut data_lines: Vec<&str> = Vec::new();
        for line in raw_event.lines() {
            if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(rest.trim_start());
            }
            // We ignore `event:` lines — the JSON payload carries
            // the `type` discriminator that we need anyway.
        }
        if data_lines.is_empty() {
            continue;
        }
        let payload = data_lines.join("\n");
        let v: serde_json::Value = match serde_json::from_str(&payload) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match kind {
            "content_block_start" => {
                if let Some(block) = v.get("content_block") {
                    let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if block_type == "tool_use" {
                        let id = block
                            .get("id")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = block
                            .get("name")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string();
                        current_tool_id = Some((id, name));
                    } else {
                        current_tool_id = None;
                    }
                }
            }
            "content_block_delta" => {
                let Some(delta) = v.get("delta") else {
                    continue;
                };
                let delta_type = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match delta_type {
                    "text_delta" => {
                        if let Some(text) = delta.get("text").and_then(|s| s.as_str()) {
                            deltas.push(MessageDelta::TextDelta {
                                text: text.to_string(),
                            });
                        }
                    }
                    "input_json_delta" => {
                        if let (Some((id, name)), Some(partial)) = (
                            current_tool_id.as_ref(),
                            delta.get("partial_json").and_then(|s| s.as_str()),
                        ) {
                            deltas.push(MessageDelta::ToolUseDelta {
                                id: id.clone(),
                                name: name.clone(),
                                input_partial: partial.to_string(),
                            });
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                current_tool_id = None;
            }
            "message_delta" => {
                if let Some(stop) = v
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(|s| s.as_str())
                {
                    deltas.push(MessageDelta::Done {
                        stop_reason: stop.to_string(),
                    });
                }
            }
            "message_stop" => {
                // If no `message_delta` carried a `stop_reason`, we
                // still want a terminal event for downstream
                // budget-checking. Emit `Done` with an empty reason.
                let has_done = deltas
                    .iter()
                    .any(|d| matches!(d, MessageDelta::Done { .. }));
                if !has_done {
                    deltas.push(MessageDelta::Done {
                        stop_reason: String::new(),
                    });
                }
            }
            _ => {}
        }
    }

    (deltas, tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_text_delta_event() {
        let body = "\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\
\n";
        let (deltas, tail) = parse_anthropic_sse(body);
        assert_eq!(deltas.len(), 1);
        assert!(matches!(&deltas[0], MessageDelta::TextDelta { text } if text == "hi"));
        assert!(tail.is_empty());
    }

    #[test]
    fn parse_two_text_deltas_into_concatenable_pieces() {
        let body = "\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\
\n";
        let (deltas, _) = parse_anthropic_sse(body);
        let joined: String = deltas
            .iter()
            .filter_map(|d| match d {
                MessageDelta::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(joined, "Hello world");
    }

    #[test]
    fn parse_message_delta_emits_done_with_reason() {
        let body = "\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\
\n";
        let (deltas, _) = parse_anthropic_sse(body);
        assert_eq!(deltas.len(), 1);
        assert!(
            matches!(&deltas[0], MessageDelta::Done { stop_reason } if stop_reason == "end_turn")
        );
    }

    #[test]
    fn parse_message_stop_emits_done_when_no_prior_done() {
        let body = "\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";
        let (deltas, _) = parse_anthropic_sse(body);
        assert_eq!(deltas.len(), 1);
        assert!(matches!(&deltas[0], MessageDelta::Done { .. }));
    }

    #[test]
    fn parse_tail_returned_when_event_unterminated() {
        let body = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\"";
        let (deltas, tail) = parse_anthropic_sse(body);
        assert!(deltas.is_empty());
        assert_eq!(tail, body);
    }

    /// v0.27 Track E (QoL #2): the source-level `next()` surface
    /// yields the same deltas as the underlying `Stream` impl, and
    /// returns `None` after the stream is exhausted.
    #[tokio::test]
    async fn next_yields_deltas_then_none() {
        let deltas = vec![
            Ok(MessageDelta::TextDelta { text: "hi".into() }),
            Ok(MessageDelta::TextDelta {
                text: "there".into(),
            }),
            Ok(MessageDelta::Done {
                stop_reason: "end_turn".into(),
            }),
        ];
        let mut s = MessageStream::from_vec(deltas);
        assert!(matches!(s.next().await, Some(MessageDelta::TextDelta { text }) if text == "hi"));
        assert!(
            matches!(s.next().await, Some(MessageDelta::TextDelta { text }) if text == "there")
        );
        assert!(matches!(
            s.next().await,
            Some(MessageDelta::Done { stop_reason }) if stop_reason == "end_turn"
        ));
        assert!(s.next().await.is_none());
    }

    /// `next_blocking` exposes the same iteration shape from a
    /// non-async caller (the SIR interpreter's eval_method).
    #[test]
    fn next_blocking_yields_deltas_then_none() {
        let deltas = vec![
            Ok(MessageDelta::TextDelta {
                text: "alpha".into(),
            }),
            Ok(MessageDelta::Done {
                stop_reason: "end_turn".into(),
            }),
        ];
        let mut s = MessageStream::from_vec(deltas);
        assert!(
            matches!(s.next_blocking(), Some(MessageDelta::TextDelta { text }) if text == "alpha")
        );
        assert!(matches!(s.next_blocking(), Some(MessageDelta::Done { .. })));
        assert!(s.next_blocking().is_none());
    }

    /// Stream errors collapse to a `Done` with `stream_error:` so the
    /// `next()` API never panics or wedges its caller.
    #[tokio::test]
    async fn next_collapses_stream_errors_to_done() {
        let deltas = vec![Err(crate::llm::error::LlmError::Transport("boom".into()))];
        let mut s = MessageStream::from_vec(deltas);
        let got = s.next().await;
        assert!(matches!(
            got,
            Some(MessageDelta::Done { stop_reason }) if stop_reason.starts_with("stream_error:")
        ));
    }

    #[test]
    fn parse_tool_use_input_json_delta_stitches_to_tool_use_delta() {
        let body = "\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_01\",\"name\":\"search\",\"input\":{}}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"q\\\":\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"rust\\\"}\"}}\n\
\n";
        let (deltas, _) = parse_anthropic_sse(body);
        let mut joined = String::new();
        let mut id = String::new();
        let mut name = String::new();
        for d in &deltas {
            if let MessageDelta::ToolUseDelta {
                id: i,
                name: n,
                input_partial,
            } = d
            {
                id = i.clone();
                name = n.clone();
                joined.push_str(input_partial);
            }
        }
        assert_eq!(id, "toolu_01");
        assert_eq!(name, "search");
        assert_eq!(joined, "{\"q\":\"rust\"}");
    }
}
