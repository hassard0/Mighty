//! Google Gemini `generateContent` client — v0.27 **full**.
//!
//! POST `https://generativelanguage.googleapis.com/v1beta/models/<MODEL>:generateContent?key=<KEY>`
//! with the typed shapes from [`crate::llm::message`] +
//! [`crate::llm::tools`]. Promoted from v0.26 skeleton: now ships
//! HTTP/1.1 + `streamGenerateContent?alt=sse` streaming + function
//! calling + safety settings + budget short-circuit.
//!
//! ## Auth
//!
//! `GEMINI_API_KEY` env var (or `GOOGLE_API_KEY` — checked as a
//! fallback). The key rides on the URL as `?key=<key>` per Google's
//! convention; the body is `application/json`.
//!
//! ## Streaming
//!
//! Gemini exposes two streaming wire shapes:
//!
//! 1. Default `:streamGenerateContent` — emits a top-level JSON array
//!    that the server flushes one element at a time. Annoying to
//!    parse incrementally.
//! 2. `:streamGenerateContent?alt=sse` — proper SSE with one
//!    `GenerateContentResponse` JSON per `data:` line.
//!
//! We use shape 2. The free-function [`parse_gemini_sse`] handles the
//! event split + projection onto [`MessageDelta`].
//!
//! ## Role mapping
//!
//! Gemini uses `model` instead of `assistant` for the model's turn.
//! `system` is hoisted out of the messages list into a top-level
//! `systemInstruction` field. `tool` is normalised into `user` with a
//! `functionResponse` part.

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
use crate::llm::observe_record;
use crate::llm::provider::{CompletionRequest, LlmProvider};
use crate::llm::streaming::MessageStream;
use crate::llm::tools::{Tool, ToolChoice};

#[derive(Debug, Clone)]
pub struct GeminiClient {
    api_key: String,
    base_url: String,
    /// Optional safety-setting overrides; default is the Google
    /// "BLOCK_NONE" preset across the four canonical categories so
    /// developer prompts that contain code or technical strings don't
    /// trip the default thresholds.
    safety_settings: Option<serde_json::Value>,
}

impl GeminiClient {
    /// v0.29 Track E: also consults `GEMINI_BASE_URL` (or the universal
    /// `MTY_LLM_BASE_URL` fallback) for the API base URL — see
    /// [`crate::llm::resolve_base_url`].
    pub fn from_env() -> Result<Self, LlmError> {
        let key = std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .map_err(|_| LlmError::Auth("GEMINI_API_KEY / GOOGLE_API_KEY not set".into()))?;
        let base_url = crate::llm::resolve_base_url(
            "GEMINI_BASE_URL",
            "https://generativelanguage.googleapis.com",
        );
        Ok(Self::with_api_key(key).with_base_url(base_url))
    }

    pub fn with_api_key(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://generativelanguage.googleapis.com".into(),
            safety_settings: None,
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Current base URL — defaults to `https://generativelanguage.googleapis.com`,
    /// overridden by [`with_base_url`] or by the `GEMINI_BASE_URL` /
    /// `MTY_LLM_BASE_URL` env vars when constructed via [`from_env`].
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Override the safety-settings array. Pass a JSON array of
    /// `{category, threshold}` objects.
    #[must_use]
    pub fn with_safety_settings(mut self, settings: serde_json::Value) -> Self {
        self.safety_settings = Some(settings);
        self
    }

    /// Map a model name + `(stream?)` to the `:generateContent` or
    /// `:streamGenerateContent?alt=sse` URL.
    pub fn model_endpoint(&self, model: &str, stream: bool) -> String {
        let base = self.base_url.trim_end_matches('/');
        if stream {
            format!(
                "{base}/v1beta/models/{model}:streamGenerateContent?alt=sse&key={}",
                self.api_key
            )
        } else {
            format!(
                "{base}/v1beta/models/{model}:generateContent?key={}",
                self.api_key
            )
        }
    }

    /// Build the request body. Mirrors the Anthropic `build_body` so
    /// tests can pin the wire shape without hitting the network.
    pub fn build_body(&self, req: &CompletionRequest) -> serde_json::Value {
        let contents: Vec<serde_json::Value> = req.messages.iter().map(message_to_gemini).collect();
        let mut body = serde_json::json!({ "contents": contents });
        if let Some(sys) = &req.system {
            body["systemInstruction"] = serde_json::json!({
                "parts": [{ "text": sys }]
            });
        }
        if !req.tools.is_empty() {
            body["tools"] = serde_json::json!([{
                "functionDeclarations": req.tools.iter().map(tool_to_gemini).collect::<Vec<_>>()
            }]);
        }
        if let Some(choice) = &req.tool_choice {
            body["toolConfig"] = tool_choice_to_gemini(choice);
        }
        let mut gen_config = serde_json::Map::new();
        if let Some(t) = req.temperature {
            gen_config.insert("temperature".into(), serde_json::json!(t));
        }
        if let Some(n) = req.max_tokens {
            gen_config.insert("maxOutputTokens".into(), serde_json::json!(n));
        }
        if !gen_config.is_empty() {
            body["generationConfig"] = serde_json::Value::Object(gen_config);
        }
        if let Some(settings) = &self.safety_settings {
            body["safetySettings"] = settings.clone();
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

/// Serialise one [`Message`] into Gemini's `Content` shape.
fn message_to_gemini(m: &Message) -> serde_json::Value {
    let role = match m.role {
        Role::Assistant => "model",
        // Tool-result messages use `user` role per Gemini convention;
        // the part itself carries the `functionResponse` tag.
        _ => "user",
    };
    let parts: Vec<serde_json::Value> = m
        .content
        .iter()
        .map(|b| match b {
            ContentBlock::Text { text } => serde_json::json!({ "text": text }),
            ContentBlock::ToolUse(tu) => serde_json::json!({
                "functionCall": {
                    "name": tu.name,
                    "args": tu.input,
                }
            }),
            ContentBlock::ToolResult(tr) => {
                // Gemini distinguishes by the function name, not the id.
                // We don't carry the name through ToolResult, so we
                // best-effort use the tool_use_id as a fallback name.
                serde_json::json!({
                    "functionResponse": {
                        "name": tr.tool_use_id,
                        "response": { "content": tr.content },
                    }
                })
            }
            ContentBlock::Image { source } => match source {
                crate::llm::message::ImageSource::Base64 { media_type, data } => {
                    serde_json::json!({
                        "inlineData": { "mimeType": media_type, "data": data }
                    })
                }
                crate::llm::message::ImageSource::Url { url } => {
                    serde_json::json!({
                        "fileData": { "fileUri": url }
                    })
                }
            },
        })
        .collect();
    serde_json::json!({ "role": role, "parts": parts })
}

fn tool_to_gemini(t: &Tool) -> serde_json::Value {
    serde_json::json!({
        "name": t.name,
        "description": t.description,
        "parameters": t.input_schema,
    })
}

fn tool_choice_to_gemini(c: &ToolChoice) -> serde_json::Value {
    let mode = match c {
        ToolChoice::Auto => "AUTO",
        ToolChoice::Any => "ANY",
        ToolChoice::None => "NONE",
        // Gemini doesn't have a "must call THIS tool" mode; map to ANY
        // and let the upstream pick from the allowed list (which the
        // caller can scope by restricting `tools` to just this one).
        ToolChoice::Tool { .. } => "ANY",
    };
    let mut cfg = serde_json::json!({
        "functionCallingConfig": { "mode": mode }
    });
    if let ToolChoice::Tool { name } = c {
        cfg["functionCallingConfig"]["allowedFunctionNames"] = serde_json::json!([name]);
    }
    cfg
}

#[async_trait::async_trait]
impl LlmProvider for GeminiClient {
    async fn complete(&self, req: CompletionRequest) -> Result<Message, LlmError> {
        // v0.30 Track D — std.observe hook (see anthropic.rs).
        let observe_started = std::time::Instant::now();
        let observe_started_at_ms = crate::observe::observation::now_ms();
        Self::check_budgets_pre(&req)?;
        if self.api_key.is_empty() {
            return Err(LlmError::Auth("Gemini api key empty".into()));
        }
        let url = self.model_endpoint(&req.model, false);
        let body = self.build_body(&req);
        let body_bytes = serde_json::to_vec(&body)?;

        let resp = match http_post(&url, &[("content-type", "application/json")], body_bytes).await
        {
            Ok(r) => r,
            Err(e) => {
                observe_record(
                    "gemini",
                    &req.model,
                    0,
                    0,
                    observe_started.elapsed().as_millis() as u64,
                    observe_started_at_ms,
                    Some("transport"),
                );
                return Err(e);
            }
        };

        match resp.status {
            200 => {}
            401 | 403 => {
                observe_record(
                    "gemini",
                    &req.model,
                    0,
                    0,
                    observe_started.elapsed().as_millis() as u64,
                    observe_started_at_ms,
                    Some("auth"),
                );
                return Err(LlmError::Auth(format!(
                    "gemini rejected api key ({})",
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
                observe_record(
                    "gemini",
                    &req.model,
                    0,
                    0,
                    observe_started.elapsed().as_millis() as u64,
                    observe_started_at_ms,
                    Some("rate_limit"),
                );
                return Err(LlmError::RateLimit(RateLimitError::new(retry, msg)));
            }
            other => {
                let body = String::from_utf8_lossy(&resp.body).to_string();
                observe_record(
                    "gemini",
                    &req.model,
                    0,
                    0,
                    observe_started.elapsed().as_millis() as u64,
                    observe_started_at_ms,
                    Some("provider"),
                );
                return Err(LlmError::Provider {
                    status: other,
                    body,
                });
            }
        }

        let parsed: GenerateContentResponse =
            serde_json::from_slice(&resp.body).map_err(|e| LlmError::Decode(e.to_string()))?;

        let (input_tokens, output_tokens) = parsed
            .usage_metadata
            .as_ref()
            .map(|u| (u.prompt_token_count, u.candidates_token_count))
            .unwrap_or((0, 0));

        let blocks = parsed.into_blocks();
        let msg = Message {
            role: Role::Assistant,
            content: blocks,
        };

        observe_record(
            "gemini",
            &req.model,
            input_tokens,
            output_tokens,
            observe_started.elapsed().as_millis() as u64,
            observe_started_at_ms,
            None,
        );

        Self::account_usage(&req, input_tokens, output_tokens)?;

        Ok(msg)
    }

    async fn complete_stream(&self, req: CompletionRequest) -> Result<MessageStream, LlmError> {
        Self::check_budgets_pre(&req)?;
        if self.api_key.is_empty() {
            return Err(LlmError::Auth("Gemini api key empty".into()));
        }
        let url = self.model_endpoint(&req.model, true);
        let body = self.build_body(&req);
        let body_bytes = serde_json::to_vec(&body)?;

        let stream = http_post_stream(
            &url,
            &[
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
                        let (deltas, new_tail) = parse_gemini_sse(&combined);
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
        tool_to_gemini(tool)
    }
}

// ---------------------------------------------------------------
// Gemini wire shape — kept private.
// ---------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GenerateContentResponse {
    #[serde(default)]
    candidates: Vec<RawCandidate>,
    #[serde(default, rename = "usageMetadata")]
    usage_metadata: Option<RawUsage>,
}

#[derive(Debug, Deserialize)]
struct RawCandidate {
    content: Option<RawContent>,
    #[allow(dead_code)]
    #[serde(default, rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawContent {
    #[allow(dead_code)]
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    parts: Vec<RawPart>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPart {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    function_call: Option<RawFunctionCall>,
}

#[derive(Debug, Deserialize)]
struct RawFunctionCall {
    name: String,
    #[serde(default)]
    args: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawUsage {
    #[serde(default)]
    prompt_token_count: u64,
    #[serde(default)]
    candidates_token_count: u64,
}

impl GenerateContentResponse {
    fn into_blocks(self) -> Vec<ContentBlock> {
        let mut blocks = Vec::new();
        // Gemini packs the entire assistant turn into the first
        // candidate's content.parts list.
        for cand in self.candidates {
            let Some(content) = cand.content else {
                continue;
            };
            for part in content.parts {
                if let Some(text) = part.text {
                    blocks.push(ContentBlock::Text { text });
                }
                if let Some(fc) = part.function_call {
                    // Gemini doesn't issue ids for function calls;
                    // synthesise from `name + index` so tool_result
                    // pairing has something stable to bind against.
                    let id = format!("gem_{}_{}", fc.name, blocks.len());
                    blocks.push(ContentBlock::ToolUse(ToolUse {
                        id,
                        name: fc.name,
                        input: fc.args,
                    }));
                }
            }
        }
        blocks
    }
}

// ---------------------------------------------------------------
// SSE parser for `streamGenerateContent?alt=sse`.
// ---------------------------------------------------------------

/// Parse a buffer of Gemini SSE bytes into typed deltas.
///
/// Each SSE `data:` line carries a `GenerateContentResponse` JSON.
/// The terminal element has a non-null `finishReason` on its first
/// candidate; we emit `MessageDelta::Done` when we see that.
pub fn parse_gemini_sse(input: &str) -> (Vec<MessageDelta>, String) {
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

    for raw_event in complete.split("\n\n") {
        let mut data_lines: Vec<&str> = Vec::new();
        for line in raw_event.lines() {
            if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(rest.trim_start());
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
        // Each chunk is a GenerateContentResponse. Walk candidates[0].content.parts.
        let candidates = v.get("candidates").and_then(|c| c.as_array());
        if let Some(cands) = candidates {
            for cand in cands {
                if let Some(parts) = cand
                    .get("content")
                    .and_then(|c| c.get("parts"))
                    .and_then(|p| p.as_array())
                {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(|s| s.as_str()) {
                            if !text.is_empty() {
                                deltas.push(MessageDelta::TextDelta {
                                    text: text.to_string(),
                                });
                            }
                        }
                        if let Some(fc) = part.get("functionCall") {
                            let name = fc
                                .get("name")
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_string();
                            // Gemini sends function-call args in one
                            // shot per chunk (not fragmented). Stitch
                            // anyway by emitting a single ToolUseDelta
                            // with the full args as one partial — the
                            // downstream stitching loop is a no-op.
                            let args = fc
                                .get("args")
                                .map(|a| a.to_string())
                                .unwrap_or_else(|| "{}".into());
                            deltas.push(MessageDelta::ToolUseDelta {
                                id: format!("gem_{name}"),
                                name,
                                input_partial: args,
                            });
                        }
                    }
                }
                if let Some(fr) = cand.get("finishReason").and_then(|s| s.as_str()) {
                    if !fr.is_empty() && fr != "null" {
                        deltas.push(MessageDelta::Done {
                            stop_reason: fr.to_string(),
                        });
                    }
                }
            }
        }
    }

    (deltas, tail)
}

// ---------------------------------------------------------------
// Mini HTTPS client. Inlined per provider.
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
                "gemini rejected api key ({status})"
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
    fn endpoint_includes_model_and_stream_verb() {
        let c = GeminiClient::with_api_key("k");
        let one_shot = c.model_endpoint("gemini-2.5-pro", false);
        let streaming = c.model_endpoint("gemini-2.5-pro", true);
        assert!(one_shot.contains(":generateContent"));
        assert!(streaming.contains(":streamGenerateContent"));
        assert!(streaming.contains("alt=sse"));
        assert!(one_shot.contains("gemini-2.5-pro"));
    }

    #[test]
    fn assistant_role_maps_to_model() {
        let c = GeminiClient::with_api_key("k");
        let req = CompletionRequest::new("gemini-2.5-pro", vec![Message::assistant_text("hello")]);
        let body = c.build_body(&req);
        assert_eq!(body["contents"][0]["role"], "model");
        assert_eq!(body["contents"][0]["parts"][0]["text"], "hello");
    }

    #[test]
    fn system_prompt_hoisted_to_system_instruction() {
        let c = GeminiClient::with_api_key("k");
        let req = CompletionRequest::new("gemini-2.5-pro", vec![Message::user_text("hi")])
            .with_system("be brief");
        let body = c.build_body(&req);
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "be brief");
    }

    #[test]
    fn safety_settings_round_trip_into_body() {
        let c = GeminiClient::with_api_key("k").with_safety_settings(serde_json::json!([
            { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_NONE" }
        ]));
        let req = CompletionRequest::new("gemini-2.5-pro", vec![Message::user_text("x")]);
        let body = c.build_body(&req);
        assert_eq!(
            body["safetySettings"][0]["category"],
            "HARM_CATEGORY_DANGEROUS_CONTENT"
        );
    }

    #[test]
    fn parse_sse_text_part_emits_text_delta() {
        let body = "\
data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"hi\"}]}}]}\n\
\n";
        let (deltas, _) = parse_gemini_sse(body);
        assert_eq!(deltas.len(), 1);
        assert!(matches!(&deltas[0], MessageDelta::TextDelta { text } if text == "hi"));
    }

    #[test]
    fn parse_sse_finish_reason_emits_done() {
        let body = "\
data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"\"}]},\"finishReason\":\"STOP\"}]}\n\
\n";
        let (deltas, _) = parse_gemini_sse(body);
        assert!(deltas
            .iter()
            .any(|d| matches!(d, MessageDelta::Done { stop_reason } if stop_reason == "STOP")));
    }

    // -------------------------------------------------------------------------
    // v0.32 Track F: structural tool_use parsing through Gemini's
    // `function_call` parts. Gemini doesn't issue tool ids itself, so
    // the lift synthesises a stable id from `gem_<name>_<index>`.

    #[test]
    fn gemini_response_lifts_function_call_into_tool_use_block() {
        let raw = serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [
                        {"text": "let me check"},
                        {"functionCall": {"name": "search_web", "args": {"q": "rust"}}}
                    ]
                }
            }]
        });
        let parsed: GenerateContentResponse = serde_json::from_value(raw).unwrap();
        let blocks = parsed.into_blocks();
        let msg = Message {
            role: Role::Assistant,
            content: blocks,
        };
        assert_eq!(msg.text(), "let me check");
        let tus = msg.tool_uses();
        assert_eq!(tus.len(), 1);
        assert_eq!(tus[0].name, "search_web");
        assert_eq!(tus[0].input["q"], "rust");
        // Gemini-synthesised id pattern.
        assert!(tus[0].id.starts_with("gem_search_web_"));
    }

    #[test]
    fn gemini_response_with_no_function_calls_yields_no_tool_uses() {
        let raw = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{"text": "hello"}]
                }
            }]
        });
        let parsed: GenerateContentResponse = serde_json::from_value(raw).unwrap();
        let blocks = parsed.into_blocks();
        let msg = Message {
            role: Role::Assistant,
            content: blocks,
        };
        assert_eq!(msg.text(), "hello");
        assert!(msg.tool_uses().is_empty());
    }
}
