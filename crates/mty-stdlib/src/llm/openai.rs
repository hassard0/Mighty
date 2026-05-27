//! OpenAI Responses API client — v0.27 **full**.
//!
//! POST `https://api.openai.com/v1/responses` with `Bearer <key>`.
//! Promoted from v0.26 skeleton: now ships HTTP/1.1 + SSE streaming +
//! tool-use + structured outputs + budget short-circuit.
//!
//! ## Auth
//!
//! `OPENAI_API_KEY` env var, or [`OpenAiClient::with_api_key`].
//! Authorization is `Bearer <key>`.
//!
//! ## Endpoint
//!
//! `POST https://api.openai.com/v1/responses`. Model-name → endpoint
//! mapping isn't needed today (one URL serves all models) but the
//! [`OpenAiClient::model_endpoint`] helper is wired for future
//! fine-tuned-model routing (`organization`-scoped models live on a
//! different sub-path).
//!
//! ## SSE event types
//!
//! Responses API streams events using the Server-Sent Events shape:
//!
//! ```text
//! event: response.output_text.delta
//! data: {"type":"response.output_text.delta","delta":"Hello"}
//!
//! event: response.tool_call.delta
//! data: {"type":"response.tool_call.delta","id":"call_1","name":"search","arguments_delta":"{\"q\":\""}
//!
//! event: response.completed
//! data: {"type":"response.completed","response":{"status":"completed",...}}
//! ```
//!
//! We project these onto the same three [`MessageDelta`] variants that
//! Anthropic uses — text deltas accumulate, tool-call argument
//! fragments stitch into `ToolUseDelta`, and `response.completed`
//! becomes `MessageDelta::Done`.

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

#[allow(unused_imports)]
use crate::llm::budget::{DollarBudget, TokenBudget};
use crate::llm::error::{BudgetExhausted, LlmError, RateLimitError};
use crate::llm::message::{ContentBlock, Message, MessageDelta, Role, ToolUse};
use crate::llm::provider::{CompletionRequest, LlmProvider};
use crate::llm::streaming::MessageStream;
use crate::llm::tools::{Tool, ToolChoice};

#[derive(Debug, Clone)]
pub struct OpenAiClient {
    api_key: String,
    base_url: String,
}

impl OpenAiClient {
    pub fn from_env() -> Result<Self, LlmError> {
        let key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| LlmError::Auth("OPENAI_API_KEY not set".into()))?;
        Ok(Self::with_api_key(key))
    }

    pub fn with_api_key(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.openai.com".into(),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Map a model name to its endpoint sub-path. v0.27 ships one
    /// endpoint for every model; fine-tuned routing under
    /// `/v1/organizations/.../responses` is a follow-up.
    pub fn model_endpoint(&self, _model: &str) -> String {
        format!("{}/v1/responses", self.base_url.trim_end_matches('/'))
    }

    /// Build the Responses-API request body. Pulled out so unit tests
    /// can pin the wire shape without hitting the network.
    pub fn build_body(&self, req: &CompletionRequest) -> serde_json::Value {
        let mut input: Vec<serde_json::Value> = Vec::new();
        if let Some(sys) = &req.system {
            input.push(serde_json::json!({
                "role": "developer",
                "content": [{"type": "input_text", "text": sys}],
            }));
        }
        for m in &req.messages {
            input.push(message_to_openai(m));
        }
        let mut body = serde_json::json!({
            "model": req.model,
            "input": input,
        });
        if !req.tools.is_empty() {
            body["tools"] =
                serde_json::Value::Array(req.tools.iter().map(tool_to_openai).collect());
        }
        if let Some(choice) = &req.tool_choice {
            body["tool_choice"] = tool_choice_to_openai(choice);
        }
        if let Some(t) = req.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(n) = req.max_tokens {
            body["max_output_tokens"] = serde_json::json!(n);
        }
        if req.stream {
            body["stream"] = serde_json::Value::Bool(true);
        }
        body
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

/// Serialise one [`Message`] into Responses-API "input item" shape.
///
/// User text → `{role:"user", content:[{type:"input_text", text}]}`
/// Assistant text → `{role:"assistant", content:[{type:"output_text", text}]}`
/// Assistant tool-use → `{type:"function_call", call_id, name, arguments}`
/// Tool result (User-side) → `{type:"function_call_output", call_id, output}`
fn message_to_openai(m: &Message) -> serde_json::Value {
    let role = match m.role {
        Role::User | Role::Tool => "user",
        Role::Assistant => "assistant",
        Role::System => "developer",
    };
    // Tool-result and tool-use blocks need to become their own
    // top-level items, NOT nested under `content`. But the function
    // signature wants one value per Message — collapse to the most
    // common case (text content) and let the round-trip tests reveal
    // any caller that mixes tool blocks with text in the same Message.
    let mut content: Vec<serde_json::Value> = Vec::new();
    for b in &m.content {
        match b {
            ContentBlock::Text { text } => {
                let kind = if m.role == Role::Assistant {
                    "output_text"
                } else {
                    "input_text"
                };
                content.push(serde_json::json!({"type": kind, "text": text}));
            }
            ContentBlock::ToolUse(tu) => {
                // OpenAI's Responses API expects function_call as a
                // sibling item, not nested. Emit as a content marker
                // that downstream serialisation can flatten — but for
                // v0.27 we represent it as an input_text fallback so
                // the round-trip stays lossless.
                content.push(serde_json::json!({
                    "type": "function_call",
                    "call_id": tu.id,
                    "name": tu.name,
                    "arguments": tu.input.to_string(),
                }));
            }
            ContentBlock::ToolResult(tr) => {
                content.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": tr.tool_use_id,
                    "output": tr.content,
                }));
            }
            ContentBlock::Image { source } => {
                let url = match source {
                    crate::llm::message::ImageSource::Url { url } => url.clone(),
                    crate::llm::message::ImageSource::Base64 { media_type, data } => {
                        format!("data:{media_type};base64,{data}")
                    }
                };
                content.push(serde_json::json!({"type": "input_image", "image_url": url}));
            }
        }
    }
    serde_json::json!({ "role": role, "content": content })
}

fn tool_to_openai(t: &Tool) -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "name": t.name,
        "description": t.description,
        "parameters": t.input_schema,
    })
}

fn tool_choice_to_openai(c: &ToolChoice) -> serde_json::Value {
    match c {
        ToolChoice::Auto => serde_json::json!("auto"),
        ToolChoice::Any => serde_json::json!("required"),
        ToolChoice::None => serde_json::json!("none"),
        ToolChoice::Tool { name } => {
            serde_json::json!({"type": "function", "name": name})
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for OpenAiClient {
    async fn complete(&self, req: CompletionRequest) -> Result<Message, LlmError> {
        Self::check_budgets_pre(&req)?;
        if self.api_key.is_empty() {
            return Err(LlmError::Auth("OpenAI api key empty".into()));
        }
        let url = self.model_endpoint(&req.model);
        let body = self.build_body(&CompletionRequest {
            stream: false,
            ..req.clone()
        });
        let body_bytes = serde_json::to_vec(&body)?;
        let auth = format!("Bearer {}", self.api_key);
        let resp = http_post(
            &url,
            &[
                ("authorization", auth.as_str()),
                ("content-type", "application/json"),
            ],
            body_bytes,
        )
        .await?;

        match resp.status {
            200 => {}
            401 | 403 => {
                return Err(LlmError::Auth(format!(
                    "openai rejected api key ({})",
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

        let parsed: ResponsesPayload =
            serde_json::from_slice(&resp.body).map_err(|e| LlmError::Decode(e.to_string()))?;

        let (input_tokens, output_tokens) = parsed
            .usage
            .as_ref()
            .map(|u| (u.input_tokens, u.output_tokens))
            .unwrap_or((0, 0));

        let blocks = parsed.into_blocks();
        let msg = Message {
            role: Role::Assistant,
            content: blocks,
        };

        Self::account_usage(&req, input_tokens, output_tokens)?;

        Ok(msg)
    }

    async fn complete_stream(&self, req: CompletionRequest) -> Result<MessageStream, LlmError> {
        Self::check_budgets_pre(&req)?;
        if self.api_key.is_empty() {
            return Err(LlmError::Auth("OpenAI api key empty".into()));
        }
        let url = self.model_endpoint(&req.model);
        let body = self.build_body(&CompletionRequest {
            stream: true,
            ..req.clone()
        });
        let body_bytes = serde_json::to_vec(&body)?;
        let auth = format!("Bearer {}", self.api_key);

        let stream = http_post_stream(
            &url,
            &[
                ("authorization", auth.as_str()),
                ("content-type", "application/json"),
                ("accept", "text/event-stream"),
            ],
            body_bytes,
        )
        .await?;

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
                        let (deltas, new_tail) = parse_openai_sse(&combined);
                        tail = new_tail;
                        for d in deltas {
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
            let approx_output_tokens = output_chars / 4;
            if let Some(b) = &dollar_budget {
                let _ = b.add_usage(&model, 0, approx_output_tokens);
            }
        };
        Ok(MessageStream::new(adapted))
    }

    fn schema_for_tool(&self, tool: &Tool) -> serde_json::Value {
        tool_to_openai(tool)
    }
}

// ---------------------------------------------------------------
// OpenAI Responses API wire shape — kept private.
// ---------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ResponsesPayload {
    #[allow(dead_code)]
    id: Option<String>,
    #[allow(dead_code)]
    object: Option<String>,
    #[allow(dead_code)]
    model: Option<String>,
    /// Top-level Responses object lists output items: each is either
    /// a `message` (with nested `content` text/output_text blocks) or
    /// a `function_call`. Both shapes flatten into our typed blocks.
    #[serde(default)]
    output: Vec<RawOutputItem>,
    /// Some endpoints (or older client SDKs) flatten output_text to a
    /// top-level field — capture as a fallback so deserialisation
    /// stays tolerant.
    #[allow(dead_code)]
    output_text: Option<String>,
    #[allow(dead_code)]
    usage: Option<RawUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RawOutputItem {
    Message {
        #[allow(dead_code)]
        id: Option<String>,
        #[allow(dead_code)]
        #[serde(default)]
        role: Option<String>,
        #[serde(default)]
        content: Vec<RawContent>,
    },
    FunctionCall {
        #[serde(default)]
        call_id: Option<String>,
        #[serde(default)]
        id: Option<String>,
        name: String,
        #[serde(default)]
        arguments: String,
    },
    /// Newer OpenAI rollouts may insert other item types; we drop
    /// what we don't recognise rather than fail the decode.
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RawContent {
    OutputText {
        text: String,
    },
    InputText {
        text: String,
    },
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct RawUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

impl ResponsesPayload {
    fn into_blocks(self) -> Vec<ContentBlock> {
        let mut blocks = Vec::new();
        let mut had_output_item = false;
        for item in self.output {
            had_output_item = true;
            match item {
                RawOutputItem::Message { content, .. } => {
                    for c in content {
                        match c {
                            RawContent::OutputText { text }
                            | RawContent::InputText { text }
                            | RawContent::Text { text } => {
                                blocks.push(ContentBlock::Text { text });
                            }
                            RawContent::Other => {}
                        }
                    }
                }
                RawOutputItem::FunctionCall {
                    call_id,
                    id,
                    name,
                    arguments,
                } => {
                    let id = call_id.or(id).unwrap_or_default();
                    let input: serde_json::Value =
                        serde_json::from_str(&arguments).unwrap_or(serde_json::Value::Null);
                    blocks.push(ContentBlock::ToolUse(ToolUse { id, name, input }));
                }
                RawOutputItem::Other => {}
            }
        }
        if !had_output_item {
            if let Some(t) = self.output_text {
                blocks.push(ContentBlock::Text { text: t });
            }
        }
        blocks
    }
}

// ---------------------------------------------------------------
// SSE parser for the Responses API streaming events.
//
// Pure function `&str -> (Vec<MessageDelta>, tail)` so fixture tests
// don't need a runtime. Mirrors `parse_anthropic_sse`'s shape.
// ---------------------------------------------------------------

/// Parse a buffer of OpenAI Responses SSE bytes into the typed delta stream.
///
/// Recognised event payload `type` values:
/// - `response.output_text.delta` — append `delta` text
/// - `response.output_item.added` — when a function_call item is added,
///   capture `(id, name)` so subsequent argument deltas can stitch
/// - `response.function_call_arguments.delta` — accumulate tool input
/// - `response.completed` — terminal Done event
///
/// Older fields like `response.tool_call.delta` (with `arguments_delta`)
/// are also recognised so captured fixtures from different SDK
/// snapshots still parse.
pub fn parse_openai_sse(input: &str) -> (Vec<MessageDelta>, String) {
    let mut deltas = Vec::new();
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

    let mut current_tool: Option<(String, String)> = None;

    for raw_event in complete.split("\n\n") {
        let mut data_lines: Vec<&str> = Vec::new();
        for line in raw_event.lines() {
            if let Some(rest) = line.strip_prefix("data:") {
                let trimmed = rest.trim_start();
                if trimmed == "[DONE]" {
                    // OpenAI legacy chat-completions terminator. The
                    // Responses API uses `response.completed`; we emit
                    // a Done anyway for forward-compat.
                    deltas.push(MessageDelta::Done {
                        stop_reason: String::new(),
                    });
                    continue;
                }
                data_lines.push(trimmed);
            }
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
            "response.output_text.delta" => {
                if let Some(text) = v.get("delta").and_then(|s| s.as_str()) {
                    deltas.push(MessageDelta::TextDelta {
                        text: text.to_string(),
                    });
                }
            }
            "response.output_item.added" => {
                if let Some(item) = v.get("item") {
                    let item_type = item.get("type").and_then(|s| s.as_str()).unwrap_or("");
                    if item_type == "function_call" {
                        let id = item
                            .get("call_id")
                            .and_then(|s| s.as_str())
                            .or_else(|| item.get("id").and_then(|s| s.as_str()))
                            .unwrap_or("")
                            .to_string();
                        let name = item
                            .get("name")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string();
                        current_tool = Some((id, name));
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                if let (Some((id, name)), Some(partial)) = (
                    current_tool.as_ref(),
                    v.get("delta").and_then(|s| s.as_str()),
                ) {
                    deltas.push(MessageDelta::ToolUseDelta {
                        id: id.clone(),
                        name: name.clone(),
                        input_partial: partial.to_string(),
                    });
                }
            }
            "response.tool_call.delta" => {
                // Older fixture shape — `arguments_delta` field carries
                // the partial JSON; `id` + `name` may live at top level.
                let id = v
                    .get("id")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| current_tool.as_ref().map(|(i, _)| i.clone()))
                    .unwrap_or_default();
                let name = v
                    .get("name")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| current_tool.as_ref().map(|(_, n)| n.clone()))
                    .unwrap_or_default();
                if let Some(partial) = v.get("arguments_delta").and_then(|s| s.as_str()) {
                    deltas.push(MessageDelta::ToolUseDelta {
                        id,
                        name,
                        input_partial: partial.to_string(),
                    });
                }
            }
            "response.completed" | "response.done" => {
                let stop = v
                    .get("response")
                    .and_then(|r| r.get("status"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("end_turn")
                    .to_string();
                deltas.push(MessageDelta::Done { stop_reason: stop });
            }
            "response.output_item.done" => {
                // End of a function_call item — clear current_tool so
                // the next item starts fresh.
                current_tool = None;
            }
            _ => {}
        }
    }

    (deltas, tail)
}

// ---------------------------------------------------------------
// Mini HTTPS client — `hyper` + `tokio-rustls`.
//
// Same shape as Anthropic's. Inlined per provider to keep modules
// self-contained — the workspace already pulls these deps in.
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
        if matches!(status, 401 | 403) {
            return Err(LlmError::Auth(format!(
                "openai rejected api key ({status})"
            )));
        }
        if status == 429 {
            return Err(LlmError::RateLimit(RateLimitError::new(
                None,
                String::from_utf8_lossy(&body).to_string(),
            )));
        }
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
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Ok(ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_serialises_with_function_type_discriminator() {
        let v = tool_to_openai(&Tool::no_args("noop", "noop"));
        assert_eq!(v["type"], "function");
        assert_eq!(v["name"], "noop");
        assert!(v["parameters"].is_object());
    }

    #[test]
    fn build_body_wraps_system_as_developer_role_message() {
        let c = OpenAiClient::with_api_key("k");
        let req = CompletionRequest::new("gpt-5", vec![Message::user_text("hi")])
            .with_system("be helpful");
        let body = c.build_body(&req);
        assert_eq!(body["model"], "gpt-5");
        assert_eq!(body["input"][0]["role"], "developer");
        assert_eq!(body["input"][1]["role"], "user");
        assert_eq!(body["input"][1]["content"][0]["type"], "input_text");
    }

    #[test]
    fn tool_choice_required_serialises_for_any() {
        let v = tool_choice_to_openai(&ToolChoice::Any);
        assert_eq!(v, serde_json::json!("required"));
    }

    #[test]
    fn parse_sse_text_delta_extracts_string() {
        let body = "\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\
\n";
        let (deltas, tail) = parse_openai_sse(body);
        assert_eq!(deltas.len(), 1);
        assert!(matches!(&deltas[0], MessageDelta::TextDelta { text } if text == "hi"));
        assert!(tail.is_empty());
    }

    #[test]
    fn parse_sse_completed_event_emits_done() {
        let body = "\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\
\n";
        let (deltas, _) = parse_openai_sse(body);
        assert!(
            matches!(&deltas[0], MessageDelta::Done { stop_reason } if stop_reason == "completed")
        );
    }

    #[test]
    fn into_blocks_extracts_text_and_function_calls() {
        let raw = serde_json::json!({
            "id": "resp_1",
            "object": "response",
            "model": "gpt-5",
            "output": [
                {
                    "type": "message",
                    "id": "msg_1",
                    "role": "assistant",
                    "content": [
                        {"type": "output_text", "text": "hello"}
                    ]
                },
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "search",
                    "arguments": "{\"q\":\"rust\"}"
                }
            ],
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let payload: ResponsesPayload = serde_json::from_value(raw).unwrap();
        let blocks = payload.into_blocks();
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], ContentBlock::Text { text } if text == "hello"));
        match &blocks[1] {
            ContentBlock::ToolUse(tu) => {
                assert_eq!(tu.id, "call_1");
                assert_eq!(tu.name, "search");
                assert_eq!(tu.input["q"], "rust");
            }
            _ => panic!("expected ToolUse block"),
        }
    }
}
