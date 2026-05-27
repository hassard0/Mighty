//! v0.27 Track E (QoL gap #2) — source-level streaming surface.
//!
//! Demo 07's `complete_stream()` works at the Rust API but a Mighty
//! `.mty` program couldn't iterate the resulting `MessageStream`
//! cleanly: the iterator shape wasn't surfaced. v0.27 lands
//! [`MessageStream::next`](mty_stdlib::llm::streaming::MessageStream::next)
//! (async) + [`MessageStream::next_blocking`](mty_stdlib::llm::streaming::MessageStream::next_blocking)
//! (sync, for the SIR interp's `eval_method` dispatch).
//!
//! The Mighty-source test is deferred until the parser allows
//! `while let Some(d) = stream.next() { ... }` cleanly against the
//! permissive-method dispatch. These Rust-side tests pin the iteration
//! contract.

use mty_stdlib::llm::message::MessageDelta;
use mty_stdlib::llm::streaming::MessageStream;

fn three_deltas() -> Vec<Result<MessageDelta, mty_stdlib::llm::error::LlmError>> {
    vec![
        Ok(MessageDelta::TextDelta {
            text: "Hello".into(),
        }),
        Ok(MessageDelta::TextDelta {
            text: " world".into(),
        }),
        Ok(MessageDelta::Done {
            stop_reason: "end_turn".into(),
        }),
    ]
}

#[tokio::test]
async fn messagestream_next_yields_deltas() {
    let mut s = MessageStream::from_vec(three_deltas());
    let mut texts = Vec::new();
    let mut saw_done = false;
    while let Some(d) = s.next().await {
        match d {
            MessageDelta::TextDelta { text } => texts.push(text),
            MessageDelta::Done { stop_reason } => {
                saw_done = true;
                assert_eq!(stop_reason, "end_turn");
            }
            _ => {}
        }
    }
    assert_eq!(texts.join(""), "Hello world");
    assert!(saw_done, "stream must terminate with a Done delta");
    // After exhaustion, next() returns None.
    assert!(s.next().await.is_none());
}

#[test]
fn messagestream_next_blocking_drives_from_sync_context() {
    let mut s = MessageStream::from_vec(three_deltas());
    let mut texts = Vec::new();
    while let Some(d) = s.next_blocking() {
        if let MessageDelta::TextDelta { text } = d {
            texts.push(text);
        }
    }
    assert_eq!(texts.join(""), "Hello world");
}

#[tokio::test]
async fn messagestream_handles_tool_use_delta() {
    // Tool-use deltas thread through the same iteration loop. The
    // Mighty source side is the canonical `match delta { ToolUse(t)
    // => ... }` arm; here we just verify the variant survives the
    // round trip.
    let stream = vec![
        Ok(MessageDelta::ToolUseDelta {
            id: "toolu_01".into(),
            name: "search".into(),
            input_partial: "{\"q\":".into(),
        }),
        Ok(MessageDelta::ToolUseDelta {
            id: "toolu_01".into(),
            name: "search".into(),
            input_partial: "\"rust\"}".into(),
        }),
        Ok(MessageDelta::Done {
            stop_reason: "tool_use".into(),
        }),
    ];
    let mut s = MessageStream::from_vec(stream);
    let mut joined = String::new();
    let mut tool_name = String::new();
    while let Some(d) = s.next().await {
        if let MessageDelta::ToolUseDelta {
            name,
            input_partial,
            ..
        } = d
        {
            tool_name = name;
            joined.push_str(&input_partial);
        }
    }
    assert_eq!(tool_name, "search");
    assert_eq!(joined, "{\"q\":\"rust\"}");
}

#[tokio::test]
async fn messagestream_collapses_errors_to_done() {
    // Per the public contract: stream errors surface as a Done with
    // a `stream_error:` prefix, so a Mighty `while let Some(d) = …`
    // loop terminates rather than wedging on `?`-style propagation
    // it can't yet express.
    let s = MessageStream::from_vec(vec![Err(mty_stdlib::llm::error::LlmError::Transport(
        "boom".into(),
    ))]);
    let mut s = s;
    let got = s.next().await;
    assert!(matches!(
        got,
        Some(MessageDelta::Done { stop_reason }) if stop_reason.starts_with("stream_error:")
    ));
    assert!(s.next().await.is_none());
}
