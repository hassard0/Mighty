//! Anthropic Messages API client.
//!
//! POST `https://api.anthropic.com/v1/messages` with the typed shapes
//! from [`crate::llm::message`] + [`crate::llm::tools`]. The full
//! `complete()` path is shipped; `complete_stream()` parses the
//! `text/event-stream` body into [`MessageStream`] via
//! [`crate::llm::streaming::parse_anthropic_sse`].
//!
//! ## Auth
//!
//! `ANTHROPIC_API_KEY` env var, or an explicit key handed to
//! [`AnthropicClient::with_api_key`]. The key goes on every request
//! as `x-api-key`. We also send `anthropic-version: 2023-06-01` which
//! is the documented stable header.
//!
//! ## HTTP backend
//!
//! Built on the workspace's existing `hyper` + `tokio-rustls`
//! stack — we wire a small HTTPS connector inline rather than pull
//! `hyper-rustls` in for one function. For testing we honour an
//! explicit `base_url` so `wiremock`'s plain HTTP listener can stand
//! in for `api.anthropic.com`.
//!
//! ## Budget integration
//!
//! `CompletionRequest::token_budget` + `dollar_budget` are checked
//! both *before* and *after* the request. On the streaming path the
//! token budget is consulted between every text-delta — once it's
//! exhausted the stream short-circuits with [`LlmError::BudgetExhausted`]
//! and no further deltas are emitted.

use std::sync::Arc;

use bytes::Bytes;
use futures_util::stream::StreamExt;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request as HReq, Uri};
use hyper_util::rt::TokioIo;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::llm::error::{BudgetExhausted, LlmError, RateLimitError};
use crate::llm::message::{ContentBlock, Message, MessageDelta, Role, ToolUse};
// Re-exported for the impl-level docs above (not all paths use both).
#[allow(unused_imports)]
use crate::llm::budget::{DollarBudget, TokenBudget};
use crate::llm::provider::{CompletionRequest, LlmProvider};
use crate::llm::streaming::{parse_anthropic_sse, MessageStream};
use crate::llm::tools::{Tool, ToolChoice};

/// The real Anthropic Messages client.
#[derive(Debug, Clone)]
pub struct AnthropicClient {
    api_key: String,
    /// Defaults to `https://api.anthropic.com`. Overridden by tests
    /// (and by self-hosted proxies) — see [`with_base_url`].
    base_url: String,
    /// `anthropic-version` header. Pinned to the documented stable
    /// version unless the caller overrides it.
    api_version: String,
}

impl AnthropicClient {
    /// New client that pulls `ANTHROPIC_API_KEY` from the process env.
    /// Returns an error if the env var is missing — callers who want
    /// to set the key explicitly should use [`with_api_key`] instead.
    ///
    /// v0.29 Track E: also consults `ANTHROPIC_BASE_URL` (or the
    /// universal `MTY_LLM_BASE_URL` fallback) for the API base URL.
    /// Production callers leave both unset and the client targets
    /// `https://api.anthropic.com`; mock-LLM tests can redirect at
    /// process-launch time without touching the code path.
    pub fn from_env() -> Result<Self, LlmError> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| LlmError::Auth("ANTHROPIC_API_KEY not set".into()))?;
        let base_url =
            crate::llm::resolve_base_url("ANTHROPIC_BASE_URL", "https://api.anthropic.com");
        Ok(Self::with_api_key(key).with_base_url(base_url))
    }

    pub fn with_api_key(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.anthropic.com".into(),
            api_version: "2023-06-01".into(),
        }
    }

    /// Point the client at a different base URL — typically a wiremock
    /// server (`http://127.0.0.1:NNNN`) in tests, or a corporate
    /// proxy in production.
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Current base URL — defaults to `https://api.anthropic.com`,
    /// overridden by [`with_base_url`] or by the `ANTHROPIC_BASE_URL` /
    /// `MTY_LLM_BASE_URL` env vars when constructed via [`from_env`].
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[must_use]
    pub fn with_api_version(mut self, version: impl Into<String>) -> Self {
        self.api_version = version.into();
        self
    }

    fn build_body(&self, req: &CompletionRequest) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = req
            .messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role.as_anthropic(),
                    "content": m.content,
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": req.model,
            "max_tokens": req.max_tokens.unwrap_or(1024),
            "messages": messages,
        });

        if let Some(system) = &req.system {
            body["system"] = serde_json::Value::String(system.clone());
        }
        if !req.tools.is_empty() {
            body["tools"] =
                serde_json::Value::Array(req.tools.iter().map(tool_to_anthropic).collect());
        }
        if let Some(choice) = &req.tool_choice {
            body["tool_choice"] = tool_choice_to_anthropic(choice);
        }
        if let Some(temp) = req.temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if req.stream {
            body["stream"] = serde_json::Value::Bool(true);
        }
        body
    }

    /// v0.30 Track C — issue a request with the Anthropic
    /// `computer_20241022` tool family enabled.
    ///
    /// This is `complete()` plus:
    ///
    /// - The `tools` array carries the `computer_20241022` spec
    ///   (`{ type, display_width_px, display_height_px,
    ///   display_number }`) instead of the generic `{ name,
    ///   description, input_schema }` triple.
    /// - The `anthropic-beta` header opts into the computer-use beta
    ///   gate so older keys without the beta access bit are rejected
    ///   with a clear 403 rather than a generic 400.
    ///
    /// The reply shape is unchanged — `Message::tool_uses()` returns
    /// the model's computer-use `tool_use` blocks for the dispatcher
    /// to parse via
    /// [`mty_stdlib::computer::dispatcher::ComputerAction::parse`](crate::computer::dispatcher::ComputerAction::parse).
    pub async fn ask_with_computer(
        &self,
        req: CompletionRequest,
        screen_size: (u32, u32),
    ) -> Result<Message, LlmError> {
        Self::check_budgets_pre(&req)?;
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        // Build the body, then swap the tools entry for the computer-
        // use spec. We start from the standard build so all the other
        // fields (system, temperature, max_tokens, …) are populated
        // consistently.
        let mut body = self.build_body(&CompletionRequest {
            stream: false,
            tools: vec![],
            ..req.clone()
        });
        body["tools"] = computer_tool_array(screen_size.0, screen_size.1);

        let body_bytes = serde_json::to_vec(&body)?;
        let resp = http_post(
            &url,
            &[
                ("x-api-key", self.api_key.as_str()),
                ("anthropic-version", self.api_version.as_str()),
                ("anthropic-beta", "computer-use-2024-10-22"),
                ("content-type", "application/json"),
            ],
            body_bytes,
        )
        .await?;

        match resp.status {
            200 => {}
            401 | 403 => {
                return Err(LlmError::Auth(format!(
                    "anthropic rejected api key ({})",
                    resp.status
                )));
            }
            429 => {
                let retry = resp
                    .headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
                    .and_then(|(_, v)| v.parse::<u64>().ok());
                let msg = String::from_utf8_lossy(&resp.body).to_string();
                return Err(LlmError::RateLimit(RateLimitError::new(retry, msg)));
            }
            other => {
                let body = String::from_utf8_lossy(&resp.body).to_string();
                return Err(LlmError::Provider {
                    status: other,
                    body,
                });
            }
        }

        let parsed: MessagesResponse =
            serde_json::from_slice(&resp.body).map_err(|e| LlmError::Decode(e.to_string()))?;
        let blocks: Vec<ContentBlock> = parsed
            .content
            .into_iter()
            .map(RawContent::into_typed)
            .collect();
        let msg = Message {
            role: Role::Assistant,
            content: blocks,
        };
        Self::account_usage(
            &req,
            parsed.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
            parsed.usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
        )?;
        Ok(msg)
    }

    fn check_budgets_pre(req: &CompletionRequest) -> Result<(), LlmError> {
        if let Some(b) = &req.token_budget {
            if b.is_exhausted() {
                return Err(LlmError::BudgetExhausted(BudgetExhausted::tokens(
                    b.limit(),
                    b.consumed(),
                )));
            }
        }
        if let Some(b) = &req.dollar_budget {
            if b.is_exhausted() {
                return Err(LlmError::BudgetExhausted(BudgetExhausted::dollars(
                    b.limit_cents(),
                    b.consumed_cents(),
                )));
            }
        }
        Ok(())
    }

    fn account_usage(
        req: &CompletionRequest,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Result<(), LlmError> {
        if let Some(b) = &req.token_budget {
            if let Err(e) = b.try_consume(input_tokens + output_tokens) {
                return Err(LlmError::BudgetExhausted(e));
            }
        }
        if let Some(b) = &req.dollar_budget {
            if let Err(e) = b.add_usage(&req.model, input_tokens, output_tokens) {
                return Err(LlmError::BudgetExhausted(e));
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl LlmProvider for AnthropicClient {
    async fn complete(&self, req: CompletionRequest) -> Result<Message, LlmError> {
        Self::check_budgets_pre(&req)?;
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let body = self.build_body(&CompletionRequest {
            stream: false,
            ..req.clone()
        });
        let body_bytes = serde_json::to_vec(&body)?;

        let resp = http_post(
            &url,
            &[
                ("x-api-key", self.api_key.as_str()),
                ("anthropic-version", self.api_version.as_str()),
                ("content-type", "application/json"),
            ],
            body_bytes,
        )
        .await?;

        match resp.status {
            200 => {}
            401 | 403 => {
                return Err(LlmError::Auth(format!(
                    "anthropic rejected api key ({})",
                    resp.status
                )));
            }
            429 => {
                let retry = resp
                    .headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
                    .and_then(|(_, v)| v.parse::<u64>().ok());
                let msg = String::from_utf8_lossy(&resp.body).to_string();
                return Err(LlmError::RateLimit(RateLimitError::new(retry, msg)));
            }
            other => {
                let body = String::from_utf8_lossy(&resp.body).to_string();
                return Err(LlmError::Provider {
                    status: other,
                    body,
                });
            }
        }

        let parsed: MessagesResponse =
            serde_json::from_slice(&resp.body).map_err(|e| LlmError::Decode(e.to_string()))?;

        // Convert the wire shape into our typed Message.
        let mut blocks = Vec::with_capacity(parsed.content.len());
        for raw in parsed.content {
            blocks.push(raw.into_typed());
        }
        let msg = Message {
            role: Role::Assistant,
            content: blocks,
        };

        // Account usage after the call so partial successes still hit
        // observability.
        Self::account_usage(
            &req,
            parsed.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
            parsed.usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
        )?;

        Ok(msg)
    }

    async fn complete_stream(&self, req: CompletionRequest) -> Result<MessageStream, LlmError> {
        Self::check_budgets_pre(&req)?;
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let body = self.build_body(&CompletionRequest {
            stream: true,
            ..req.clone()
        });
        let body_bytes = serde_json::to_vec(&body)?;

        let stream = http_post_stream(
            &url,
            &[
                ("x-api-key", self.api_key.as_str()),
                ("anthropic-version", self.api_version.as_str()),
                ("content-type", "application/json"),
                ("accept", "text/event-stream"),
            ],
            body_bytes,
        )
        .await?;

        // Adapt the raw byte stream into typed delta events with
        // budget short-circuit between each delta.
        //
        // Token accounting: we deduct *per-delta* using a rough chars
        // -> tokens estimate (~4 chars/token) so an in-flight stream
        // crosses the budget mid-flow rather than only after the
        // upstream finishes. The canonical `usage` field on the
        // terminal `message_delta` is the source of truth and overrides
        // the estimate when present.
        let token_budget = req.token_budget.clone();
        let dollar_budget = req.dollar_budget.clone();
        let model = req.model.clone();
        let adapted = async_stream::stream! {
            let mut tail = String::new();
            let mut byte_stream = stream;
            let mut output_chars: u64 = 0;
            'outer: while let Some(chunk) = byte_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        let combined = format!("{tail}{text}");
                        let (deltas, new_tail) = parse_anthropic_sse(&combined);
                        tail = new_tail;
                        for d in deltas {
                            // Deduct against the budget *first*, so
                            // BudgetExhausted lands before the next
                            // event is surfaced. We approximate
                            // tokens from char count (chars/4 + 1 so
                            // tiny strings still bill at least one
                            // token).
                            if let MessageDelta::TextDelta { text } = &d {
                                let approx = (text.len() as u64 / 4).max(1);
                                output_chars = output_chars.saturating_add(text.len() as u64);
                                if let Some(b) = &token_budget {
                                    if let Err(e) = b.try_consume(approx) {
                                        yield Err(LlmError::BudgetExhausted(e));
                                        break 'outer;
                                    }
                                }
                            }
                            // Re-check after deduction in case a
                            // sibling agent on the shared budget
                            // already tipped us over.
                            if let Some(b) = &token_budget {
                                if b.is_exhausted() {
                                    yield Err(LlmError::BudgetExhausted(
                                        BudgetExhausted::tokens(b.limit(), b.consumed()),
                                    ));
                                    break 'outer;
                                }
                            }
                            yield Ok(d);
                        }
                    }
                    Err(e) => {
                        yield Err(e);
                        break 'outer;
                    }
                }
            }
            // Dollar accounting at end-of-stream (the per-delta token
            // hits already drove the dollar budget if the caller set
            // both — but provider `usage` arrives only at the terminal
            // message_delta, so we settle output-side cents here too).
            let approx_output_tokens = output_chars / 4;
            if let Some(b) = &dollar_budget {
                let _ = b.add_usage(&model, 0, approx_output_tokens);
            }
        };

        Ok(MessageStream::new(adapted))
    }

    fn schema_for_tool(&self, tool: &Tool) -> serde_json::Value {
        tool_to_anthropic(tool)
    }
}

/// Convert a [`Tool`] into the Anthropic wire shape:
/// `{ name, description, input_schema }`.
fn tool_to_anthropic(t: &Tool) -> serde_json::Value {
    serde_json::json!({
        "name": t.name,
        "description": t.description,
        "input_schema": t.input_schema,
    })
}

fn tool_choice_to_anthropic(c: &ToolChoice) -> serde_json::Value {
    match c {
        ToolChoice::Auto => serde_json::json!({"type": "auto"}),
        ToolChoice::Any => serde_json::json!({"type": "any"}),
        ToolChoice::Tool { name } => serde_json::json!({"type": "tool", "name": name}),
        ToolChoice::None => serde_json::json!({"type": "none"}),
    }
}

/// Serialise the Anthropic `computer_20241022` tool entry. The wire
/// shape is intentionally NOT `{ name, description, input_schema }` —
/// computer-use is a provider-typed tool that Anthropic identifies by
/// its `type` discriminator, with the display dimensions inline.
pub(crate) fn computer_tool_array(width: u32, height: u32) -> serde_json::Value {
    serde_json::json!([
        {
            "type": "computer_20241022",
            "name": "computer",
            "display_width_px": width,
            "display_height_px": height,
            "display_number": 1
        }
    ])
}

// ---------------------------------------------------------------
// Anthropic wire shape — kept private; the public surface is the
// typed `Message` + `ContentBlock`.
// ---------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    #[allow(dead_code)]
    id: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "type")]
    ty: Option<String>,
    #[allow(dead_code)]
    role: Option<String>,
    content: Vec<RawContent>,
    #[allow(dead_code)]
    model: Option<String>,
    #[allow(dead_code)]
    stop_reason: Option<String>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RawContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

impl RawContent {
    fn into_typed(self) -> ContentBlock {
        match self {
            RawContent::Text { text } => ContentBlock::Text { text },
            RawContent::ToolUse { id, name, input } => {
                ContentBlock::ToolUse(ToolUse { id, name, input })
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct Usage {
    input_tokens: u64,
    output_tokens: u64,
}

// ---------------------------------------------------------------
// Mini HTTPS client — `hyper` + `tokio-rustls`.
//
// We don't pull `hyper-rustls` in just for this; the connector logic
// is small enough to inline (and the workspace already has all the
// rustls plumbing wired up via `std.tls`).
// ---------------------------------------------------------------

struct HttpResponse {
    status: u16,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
}

async fn http_post(
    url: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> Result<HttpResponse, LlmError> {
    let resp = send_request(url, headers, body).await?;
    let status = resp.status().as_u16();
    let hdrs: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let body = resp
        .into_body()
        .collect()
        .await
        .map_err(|e| LlmError::Transport(e.to_string()))?
        .to_bytes()
        .to_vec();
    Ok(HttpResponse {
        status,
        body,
        headers: hdrs,
    })
}

/// Stream variant: returns a `Stream<Item = Result<Bytes, LlmError>>`
/// of the HTTP response body chunks. The caller is responsible for
/// parsing the SSE.
async fn http_post_stream(
    url: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> Result<Box<dyn futures_core::Stream<Item = Result<Bytes, LlmError>> + Send + Unpin>, LlmError>
{
    let resp = send_request(url, headers, body).await?;
    let status = resp.status().as_u16();
    if status != 200 {
        let body = resp
            .into_body()
            .collect()
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))?
            .to_bytes()
            .to_vec();
        return Err(LlmError::Provider {
            status,
            body: String::from_utf8_lossy(&body).to_string(),
        });
    }
    let stream = async_stream::stream! {
        let mut body = resp.into_body();
        loop {
            match body.frame().await {
                None => break,
                Some(Ok(frame)) => {
                    if let Ok(data) = frame.into_data() {
                        yield Ok(data);
                    }
                }
                Some(Err(e)) => {
                    yield Err(LlmError::Transport(e.to_string()));
                    break;
                }
            }
        }
    };
    Ok(Box::new(Box::pin(stream)))
}

async fn send_request(
    url: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> Result<hyper::Response<Incoming>, LlmError> {
    let uri: Uri = url
        .parse()
        .map_err(|e: hyper::http::uri::InvalidUri| LlmError::Transport(format!("bad url: {e}")))?;
    let scheme = uri.scheme_str().unwrap_or("http");
    let host = uri
        .host()
        .ok_or_else(|| LlmError::Transport(format!("no host in {url}")))?
        .to_string();
    let port = uri
        .port_u16()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    let path_and_query = uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".into());

    let tcp = TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|e| LlmError::Transport(format!("connect {host}:{port}: {e}")))?;

    let req_builder = HReq::builder()
        .method(hyper::Method::POST)
        .uri(&path_and_query)
        .header(hyper::header::HOST, &host);
    let mut req_builder = req_builder;
    for (k, v) in headers {
        req_builder = req_builder.header(*k, *v);
    }
    let req = req_builder
        .body(Full::new(Bytes::from(body)))
        .map_err(|e| LlmError::Transport(e.to_string()))?;

    if scheme == "https" {
        let connector = TlsConnector::from(Arc::new(default_client_config()?));
        let server_name = ServerName::try_from(host.clone())
            .map_err(|e| LlmError::Transport(format!("invalid host {host}: {e}")))?;
        let stream = connector
            .connect(server_name, tcp)
            .await
            .map_err(|e| LlmError::Transport(format!("tls handshake: {e}")))?;
        let io = TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Full<Bytes>>(io)
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        sender
            .send_request(req)
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))
    } else {
        let io = TokioIo::new(tcp);
        let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Full<Bytes>>(io)
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        sender
            .send_request(req)
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))
    }
}

fn default_client_config() -> Result<ClientConfig, LlmError> {
    crate::tls::ensure_crypto_provider();
    let mut roots = RootCertStore::empty();
    // Bake the Mozilla CA bundle into the rustls config so production
    // callers actually hitting api.anthropic.com can complete the
    // handshake. Tests target wiremock over plain HTTP and never
    // touch this path.
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Ok(ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_choice_serialises_for_anthropic_tool_named() {
        let v = tool_choice_to_anthropic(&ToolChoice::Tool {
            name: "search".into(),
        });
        assert_eq!(v["type"], "tool");
        assert_eq!(v["name"], "search");
    }

    #[test]
    fn build_body_sets_max_tokens_default_to_1024() {
        let c = AnthropicClient::with_api_key("k");
        let req = CompletionRequest {
            model: "claude-opus-4-7".into(),
            messages: vec![Message::user_text("hi")],
            ..CompletionRequest::default()
        };
        let body = c.build_body(&req);
        assert_eq!(body["max_tokens"], 1024);
        assert_eq!(body["model"], "claude-opus-4-7");
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn build_body_omits_stream_when_false() {
        let c = AnthropicClient::with_api_key("k");
        let req = CompletionRequest {
            model: "m".into(),
            messages: vec![Message::user_text("x")],
            stream: false,
            ..CompletionRequest::default()
        };
        let body = c.build_body(&req);
        assert!(body.get("stream").is_none());
    }
}
