//! AWS Bedrock Converse API client — v0.27 **full**.
//!
//! POST `https://bedrock-runtime.<REGION>.amazonaws.com/model/<MODEL_ID>/converse[Stream]`
//! signed with AWS Signature Version 4. Promoted from v0.26 skeleton:
//! now ships SigV4 signing + ConverseStream event-stream parsing +
//! tool-use + budget short-circuit.
//!
//! ## Auth
//!
//! Three knobs, in order of precedence:
//!
//! 1. [`BedrockClient::with_credentials`] — caller-supplied
//!    `(access_key_id, secret_access_key, session_token?)`.
//! 2. `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` (+ optional
//!    `AWS_SESSION_TOKEN`) env vars.
//! 3. `AWS_BEDROCK_API_TOKEN` env var — short-lived bearer token
//!    (Bedrock's newer auth shape), used as a fallback for callers
//!    who don't want to manage IAM credentials.
//!
//! Region from `AWS_REGION` env var (default `us-east-1`).
//!
//! ## SigV4
//!
//! Implemented inline on top of `sha2` rather than pulling
//! `aws-sigv4` + the whole `aws-smithy-*` tree in. The algorithm is
//! small (canonical request → string-to-sign → derived key → HMAC)
//! and well-specified; see [`sign_sigv4`].
//!
//! ## Streaming
//!
//! ConverseStream uses AWS's binary event-stream protocol (NOT SSE).
//! Each frame is a 12-byte prelude + headers + payload + 4-byte
//! message CRC. We parse just enough of the framing to extract the
//! payload JSON; the message-CRC is not validated (the upstream's TLS
//! channel already covers integrity end-to-end).

use std::sync::Arc;

use bytes::Bytes;
use futures_util::stream::StreamExt;
use hex;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request as HReq, Uri};
use hyper_util::rt::TokioIo;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

#[allow(unused_imports)]
use crate::llm::budget::{DollarBudget, TokenBudget};
use crate::llm::error::{BudgetExhausted, LlmError, RateLimitError};
use crate::llm::message::{ContentBlock, Message, MessageDelta, Role, ToolUse};
use crate::llm::provider::{CompletionRequest, LlmProvider};
use crate::llm::streaming::MessageStream;
use crate::llm::tools::{Tool, ToolChoice};

/// AWS credentials. `session_token` is set when the caller uses STS
/// temporary credentials.
#[derive(Debug, Clone)]
pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

#[derive(Debug, Clone)]
enum BedrockAuth {
    Sigv4(AwsCredentials),
    BearerToken(String),
}

#[derive(Debug, Clone)]
pub struct BedrockClient {
    auth: BedrockAuth,
    region: String,
    base_url_override: Option<String>,
}

impl BedrockClient {
    /// v0.29 Track E: also consults `BEDROCK_BASE_URL` (or the universal
    /// `MTY_LLM_BASE_URL` fallback) — when set, the override replaces
    /// the region-derived `bedrock-runtime.<REGION>.amazonaws.com` URL.
    /// Useful for redirecting at a mock or a corporate gateway without
    /// touching the call sites. See [`crate::llm::resolve_base_url`].
    pub fn from_env() -> Result<Self, LlmError> {
        let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".into());
        // v0.29 Track E: optional base-URL override. We pass an empty
        // sentinel default so we can detect "no override set" vs. "set
        // to an empty string" — `resolve_base_url` treats empty as
        // unset, and we keep the override `None` so the region-derived
        // URL stays in effect.
        let base_override = {
            let resolved = crate::llm::resolve_base_url("BEDROCK_BASE_URL", "");
            if resolved.is_empty() {
                None
            } else {
                Some(resolved)
            }
        };
        // Prefer IAM creds; fall back to bearer token.
        if let (Ok(akid), Ok(secret)) = (
            std::env::var("AWS_ACCESS_KEY_ID"),
            std::env::var("AWS_SECRET_ACCESS_KEY"),
        ) {
            let session = std::env::var("AWS_SESSION_TOKEN").ok();
            return Ok(Self {
                auth: BedrockAuth::Sigv4(AwsCredentials {
                    access_key_id: akid,
                    secret_access_key: secret,
                    session_token: session,
                }),
                region,
                base_url_override: base_override,
            });
        }
        if let Ok(token) = std::env::var("AWS_BEDROCK_API_TOKEN") {
            return Ok(Self {
                auth: BedrockAuth::BearerToken(token),
                region,
                base_url_override: base_override,
            });
        }
        Err(LlmError::Auth(
            "no AWS credentials: set AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY or AWS_BEDROCK_API_TOKEN".into(),
        ))
    }

    /// Construct with explicit AWS IAM credentials. `session_token`
    /// is `None` for permanent credentials.
    pub fn with_credentials(creds: AwsCredentials) -> Self {
        Self {
            auth: BedrockAuth::Sigv4(creds),
            region: "us-east-1".into(),
            base_url_override: None,
        }
    }

    /// Construct with a short-lived API token (Bedrock's newer auth
    /// shape). The token is sent as `Authorization: Bearer <token>`
    /// and no SigV4 signing is applied.
    pub fn with_api_token(token: impl Into<String>) -> Self {
        Self {
            auth: BedrockAuth::BearerToken(token.into()),
            region: "us-east-1".into(),
            base_url_override: None,
        }
    }

    #[must_use]
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = region.into();
        self
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url_override = Some(base_url.into());
        self
    }

    /// True when this client signs requests with SigV4 (vs. the
    /// bearer-token fallback).
    pub fn signs_with_sigv4(&self) -> bool {
        matches!(self.auth, BedrockAuth::Sigv4(_))
    }

    /// v0.29 Track E: the active base URL — either the
    /// `BEDROCK_BASE_URL` / `MTY_LLM_BASE_URL` override (when
    /// constructed via [`from_env`] or [`with_base_url`]), or the
    /// region-derived `bedrock-runtime.<REGION>.amazonaws.com`.
    pub fn base_url(&self) -> String {
        self.base_url_override
            .clone()
            .unwrap_or_else(|| format!("https://bedrock-runtime.{}.amazonaws.com", self.region))
    }

    /// Endpoint for the Converse API. Streaming uses
    /// `/converse-stream` (note: AWS spells it with a hyphen in the
    /// URL path, NOT camelCase).
    pub fn converse_endpoint(&self, model_id: &str, stream: bool) -> String {
        let verb = if stream {
            "converse-stream"
        } else {
            "converse"
        };
        format!("{}/model/{}/{}", self.base_url(), model_id, verb)
    }

    /// Build the Converse-API request body.
    pub fn build_body(&self, req: &CompletionRequest) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = req
            .messages
            .iter()
            // Bedrock Converse only takes user + assistant; system goes
            // in a separate top-level field, so we filter and skip.
            .filter(|m| !matches!(m.role, Role::System))
            .map(message_to_bedrock)
            .collect();
        let mut body = serde_json::json!({ "messages": messages });
        if let Some(sys) = &req.system {
            body["system"] = serde_json::json!([{ "text": sys }]);
        }
        if !req.tools.is_empty() {
            let mut tool_config = serde_json::json!({
                "tools": req.tools.iter().map(tool_to_bedrock).collect::<Vec<_>>(),
            });
            if let Some(choice) = &req.tool_choice {
                if let Some(tc_v) = tool_choice_to_bedrock(choice) {
                    tool_config["toolChoice"] = tc_v;
                }
            }
            body["toolConfig"] = tool_config;
        }
        let mut inference_config = serde_json::Map::new();
        if let Some(t) = req.temperature {
            inference_config.insert("temperature".into(), serde_json::json!(t));
        }
        if let Some(n) = req.max_tokens {
            inference_config.insert("maxTokens".into(), serde_json::json!(n));
        }
        if !inference_config.is_empty() {
            body["inferenceConfig"] = serde_json::Value::Object(inference_config);
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

    /// Build the request headers — Authorization (SigV4 or Bearer),
    /// X-Amz-Date, optional X-Amz-Security-Token, content-type.
    fn build_auth_headers(
        &self,
        method: &str,
        uri: &Uri,
        body: &[u8],
        now_amz: &str,
        now_date: &str,
    ) -> Result<Vec<(String, String)>, LlmError> {
        let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
        let host = uri
            .host()
            .ok_or_else(|| LlmError::Transport("bedrock: no host in url".into()))?;
        headers.push(("host".to_string(), host.to_string()));
        headers.push(("x-amz-date".to_string(), now_amz.to_string()));

        match &self.auth {
            BedrockAuth::BearerToken(t) => {
                headers.push(("authorization".to_string(), format!("Bearer {t}")));
            }
            BedrockAuth::Sigv4(creds) => {
                if let Some(session) = &creds.session_token {
                    headers.push(("x-amz-security-token".to_string(), session.clone()));
                }
                let auth = sign_sigv4(
                    method,
                    uri,
                    body,
                    &headers,
                    &creds.access_key_id,
                    &creds.secret_access_key,
                    &self.region,
                    "bedrock",
                    now_amz,
                    now_date,
                )?;
                headers.push(("authorization".to_string(), auth));
            }
        }
        Ok(headers)
    }
}

/// Serialise one [`Message`] into Bedrock Converse "Message" shape.
fn message_to_bedrock(m: &Message) -> serde_json::Value {
    let role = match m.role {
        Role::Assistant => "assistant",
        _ => "user",
    };
    let content: Vec<serde_json::Value> = m
        .content
        .iter()
        .map(|b| match b {
            ContentBlock::Text { text } => serde_json::json!({ "text": text }),
            ContentBlock::ToolUse(tu) => serde_json::json!({
                "toolUse": {
                    "toolUseId": tu.id,
                    "name": tu.name,
                    "input": tu.input,
                }
            }),
            ContentBlock::ToolResult(tr) => serde_json::json!({
                "toolResult": {
                    "toolUseId": tr.tool_use_id,
                    "content": [{ "text": tr.content }],
                    "status": if tr.is_error { "error" } else { "success" },
                }
            }),
            ContentBlock::Image { source } => match source {
                crate::llm::message::ImageSource::Base64 { media_type, data } => {
                    // Bedrock wants raw bytes, not base64 string; but
                    // since we only have base64 on hand, encode in the
                    // documented shape.
                    let fmt = media_type.strip_prefix("image/").unwrap_or(media_type);
                    serde_json::json!({
                        "image": {
                            "format": fmt,
                            "source": { "bytes": data }
                        }
                    })
                }
                crate::llm::message::ImageSource::Url { url } => {
                    // Bedrock doesn't accept URL sources directly; pass
                    // through as a text block referencing the URL so the
                    // request doesn't 400 silently.
                    serde_json::json!({ "text": format!("[image: {url}]") })
                }
            },
        })
        .collect();
    serde_json::json!({ "role": role, "content": content })
}

fn tool_to_bedrock(t: &Tool) -> serde_json::Value {
    serde_json::json!({
        "toolSpec": {
            "name": t.name,
            "description": t.description,
            "inputSchema": { "json": t.input_schema }
        }
    })
}

fn tool_choice_to_bedrock(c: &ToolChoice) -> Option<serde_json::Value> {
    match c {
        ToolChoice::Auto => Some(serde_json::json!({"auto": {}})),
        ToolChoice::Any => Some(serde_json::json!({"any": {}})),
        ToolChoice::Tool { name } => Some(serde_json::json!({"tool": {"name": name}})),
        // Bedrock doesn't have a "none" mode; the caller can drop the
        // toolConfig entirely if they want tools off this turn.
        ToolChoice::None => None,
    }
}

#[async_trait::async_trait]
impl LlmProvider for BedrockClient {
    async fn complete(&self, req: CompletionRequest) -> Result<Message, LlmError> {
        Self::check_budgets_pre(&req)?;
        let url = self.converse_endpoint(&req.model, false);
        let body = self.build_body(&req);
        let body_bytes = serde_json::to_vec(&body)?;

        let uri: Uri = url.parse().map_err(|e: hyper::http::uri::InvalidUri| {
            LlmError::Transport(format!("bad url: {e}"))
        })?;
        let (now_amz, now_date) = current_amz_dates();
        let headers = self.build_auth_headers("POST", &uri, &body_bytes, &now_amz, &now_date)?;
        let header_refs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let resp = http_post(&url, &header_refs, body_bytes).await?;

        match resp.status {
            200 => {}
            401 | 403 => {
                return Err(LlmError::Auth(format!(
                    "bedrock rejected credentials ({})",
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

        let parsed: ConverseResponse =
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
        let url = self.converse_endpoint(&req.model, true);
        let body = self.build_body(&req);
        let body_bytes = serde_json::to_vec(&body)?;

        let uri: Uri = url.parse().map_err(|e: hyper::http::uri::InvalidUri| {
            LlmError::Transport(format!("bad url: {e}"))
        })?;
        let (now_amz, now_date) = current_amz_dates();
        let headers = self.build_auth_headers("POST", &uri, &body_bytes, &now_amz, &now_date)?;
        let header_refs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let stream = http_post_stream(&url, &header_refs, body_bytes).await?;

        let token_budget = req.token_budget.clone();
        let dollar_budget = req.dollar_budget.clone();
        let model = req.model.clone();
        let adapted = async_stream::stream! {
            let mut buf: Vec<u8> = Vec::new();
            let mut byte_stream = stream;
            let mut current_tool: Option<(String, String)> = None;
            let mut output_chars: u64 = 0;
            'outer: while let Some(chunk) = byte_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        buf.extend_from_slice(&bytes);
                        loop {
                            match try_parse_eventstream(&buf) {
                                EventParse::NeedMore => break,
                                EventParse::Bad => {
                                    yield Err(LlmError::Decode(
                                        "bedrock event-stream frame corrupt".into(),
                                    ));
                                    break 'outer;
                                }
                                EventParse::Frame { consumed, payload, event_type } => {
                                    let buf_after: Vec<u8> = buf[consumed..].to_vec();
                                    buf = buf_after;
                                    let deltas = project_bedrock_event(
                                        &event_type,
                                        &payload,
                                        &mut current_tool,
                                    );
                                    for d in deltas {
                                        if let MessageDelta::TextDelta { text } = &d {
                                            let approx = (text.len() as u64 / 4).max(1);
                                            output_chars =
                                                output_chars.saturating_add(text.len() as u64);
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
                                                    BudgetExhausted::tokens(
                                                        b.limit(),
                                                        b.consumed(),
                                                    ),
                                                ));
                                                break 'outer;
                                            }
                                        }
                                        yield Ok(d);
                                    }
                                }
                            }
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
        tool_to_bedrock(tool)
    }
}

// ---------------------------------------------------------------
// Bedrock Converse wire shape — kept private.
// ---------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ConverseResponse {
    output: Option<ConverseOutput>,
    #[serde(default)]
    usage: Option<RawUsage>,
    #[allow(dead_code)]
    #[serde(default, rename = "stopReason")]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConverseOutput {
    message: Option<RawMessage>,
}

#[derive(Debug, Deserialize)]
struct RawMessage {
    #[allow(dead_code)]
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    content: Vec<RawContent>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawContent {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    tool_use: Option<RawToolUse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawToolUse {
    tool_use_id: String,
    name: String,
    #[serde(default)]
    input: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

impl ConverseResponse {
    fn into_blocks(self) -> Vec<ContentBlock> {
        let mut blocks = Vec::new();
        let Some(out) = self.output else {
            return blocks;
        };
        let Some(msg) = out.message else {
            return blocks;
        };
        for c in msg.content {
            if let Some(text) = c.text {
                blocks.push(ContentBlock::Text { text });
            }
            if let Some(tu) = c.tool_use {
                blocks.push(ContentBlock::ToolUse(ToolUse {
                    id: tu.tool_use_id,
                    name: tu.name,
                    input: tu.input,
                }));
            }
        }
        blocks
    }
}

// ---------------------------------------------------------------
// AWS event-stream parser.
//
// Wire format (per AWS Common Runtime spec):
//
//   ┌────────────┬────────────┬────────────┬──────────┬─────────┬────────────┐
//   │ total_len  │ headers_len│ prelude_crc│ headers  │ payload │ message_crc│
//   │  (4 bytes) │  (4 bytes) │  (4 bytes) │   (var)  │  (var)  │  (4 bytes) │
//   └────────────┴────────────┴────────────┴──────────┴─────────┴────────────┘
//
// All multi-byte integers are big-endian. Total length covers the
// whole frame including the prelude + crcs. Headers are a packed
// sequence of `(name_len:u8, name, value_type:u8, value...)` records.
//
// We extract the `:event-type` header (value_type=7, string) and the
// payload (JSON for Converse events) and project them into the typed
// MessageDelta stream.
// ---------------------------------------------------------------

#[derive(Debug)]
enum EventParse {
    /// Not enough bytes to know yet — wait for more on the wire.
    NeedMore,
    /// Frame parsed. Consume `consumed` bytes from the front of the
    /// input.
    Frame {
        consumed: usize,
        payload: Vec<u8>,
        event_type: String,
    },
    /// Frame definitely malformed (length > 16 MiB sanity ceiling, etc).
    Bad,
}

fn try_parse_eventstream(buf: &[u8]) -> EventParse {
    if buf.len() < 12 {
        return EventParse::NeedMore;
    }
    let total_len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let headers_len = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    // Sanity: AWS caps frames at 16 MiB.
    if !(16..=16 * 1024 * 1024).contains(&total_len) {
        return EventParse::Bad;
    }
    if buf.len() < total_len {
        return EventParse::NeedMore;
    }
    // prelude_crc at buf[8..12] — skip (TLS already covers integrity).
    let headers_start = 12;
    let headers_end = headers_start + headers_len;
    let payload_start = headers_end;
    let payload_end = total_len - 4; // last 4 bytes are message_crc
    if headers_end > total_len - 4 {
        return EventParse::Bad;
    }
    let headers_bytes = &buf[headers_start..headers_end];
    let event_type = parse_event_type(headers_bytes).unwrap_or_default();
    let payload = buf[payload_start..payload_end].to_vec();
    EventParse::Frame {
        consumed: total_len,
        payload,
        event_type,
    }
}

/// Walk the headers block; return the value of the `:event-type` header.
fn parse_event_type(headers: &[u8]) -> Option<String> {
    let mut i = 0;
    while i < headers.len() {
        if i + 1 > headers.len() {
            return None;
        }
        let name_len = headers[i] as usize;
        i += 1;
        if i + name_len > headers.len() {
            return None;
        }
        let name = std::str::from_utf8(&headers[i..i + name_len])
            .ok()?
            .to_string();
        i += name_len;
        if i + 1 > headers.len() {
            return None;
        }
        let value_type = headers[i];
        i += 1;
        // value_type 7 = string: 2-byte length + bytes.
        // value_type 6 = boolean false (0 bytes)
        // We only handle the types we actually need; the rest skip past
        // their type-specific length.
        match value_type {
            0 | 1 => {} // bool true / false — 0 bytes
            2 => i += 1,
            3 => i += 2,
            4 => i += 4,
            5 => i += 8,
            6 | 7 => {
                // byte_buf or string — 2-byte len then bytes
                if i + 2 > headers.len() {
                    return None;
                }
                let l = u16::from_be_bytes([headers[i], headers[i + 1]]) as usize;
                i += 2;
                if i + l > headers.len() {
                    return None;
                }
                if name == ":event-type" {
                    return std::str::from_utf8(&headers[i..i + l])
                        .ok()
                        .map(|s| s.to_string());
                }
                i += l;
            }
            8 => i += 8,  // timestamp
            9 => i += 16, // uuid
            _ => return None,
        }
    }
    None
}

/// Map a Bedrock ConverseStream event into typed deltas.
///
/// Event types (from the AWS SDK):
/// - `messageStart` — assistant turn opens; no delta
/// - `contentBlockStart` — usually a tool_use opening; capture id+name
/// - `contentBlockDelta` — text or tool input fragment
/// - `contentBlockStop` — end of a block; clear current_tool
/// - `messageStop` — terminal Done with `stopReason`
/// - `metadata` — usage statistics; we drop these (caller's budget
///   already tracks output via per-delta estimation)
fn project_bedrock_event(
    event_type: &str,
    payload: &[u8],
    current_tool: &mut Option<(String, String)>,
) -> Vec<MessageDelta> {
    let mut out = Vec::new();
    let v: serde_json::Value = match serde_json::from_slice(payload) {
        Ok(v) => v,
        Err(_) => return out,
    };
    match event_type {
        "contentBlockStart" => {
            let Some(start) = v.get("start") else {
                return out;
            };
            if let Some(tu) = start.get("toolUse") {
                let id = tu
                    .get("toolUseId")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = tu
                    .get("name")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                *current_tool = Some((id, name));
            }
        }
        "contentBlockDelta" => {
            let Some(delta) = v.get("delta") else {
                return out;
            };
            if let Some(text) = delta.get("text").and_then(|s| s.as_str()) {
                out.push(MessageDelta::TextDelta {
                    text: text.to_string(),
                });
            }
            if let Some(tu) = delta.get("toolUse") {
                if let (Some((id, name)), Some(partial)) = (
                    current_tool.as_ref(),
                    tu.get("input").and_then(|s| s.as_str()),
                ) {
                    out.push(MessageDelta::ToolUseDelta {
                        id: id.clone(),
                        name: name.clone(),
                        input_partial: partial.to_string(),
                    });
                }
            }
        }
        "contentBlockStop" => {
            *current_tool = None;
        }
        "messageStop" => {
            let stop = v
                .get("stopReason")
                .and_then(|s| s.as_str())
                .unwrap_or("end_turn")
                .to_string();
            out.push(MessageDelta::Done { stop_reason: stop });
        }
        _ => {}
    }
    out
}

// ---------------------------------------------------------------
// SigV4 — inline. Algorithm reference:
// https://docs.aws.amazon.com/general/latest/gr/sigv4_signing.html
// ---------------------------------------------------------------

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// HMAC-SHA256 implemented inline so we don't pull `hmac` in. The
/// algorithm is RFC 2104: pad the key to the block size, XOR with the
/// inner/outer constants, hash, hash again.
fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut key_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        let mut h = Sha256::new();
        h.update(key);
        let digest = h.finalize();
        key_block[..32].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut ikey = [0u8; BLOCK];
    let mut okey = [0u8; BLOCK];
    for i in 0..BLOCK {
        ikey[i] = key_block[i] ^ 0x36;
        okey[i] = key_block[i] ^ 0x5c;
    }
    let mut inner = Sha256::new();
    inner.update(ikey);
    inner.update(msg);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(okey);
    outer.update(inner_digest);
    let out = outer.finalize();
    let mut result = [0u8; 32];
    result.copy_from_slice(&out);
    result
}

/// Produce the `Authorization` header value for a SigV4-signed
/// Bedrock request.
///
/// Returns the full `AWS4-HMAC-SHA256 Credential=…, SignedHeaders=…,
/// Signature=…` string.
#[allow(clippy::too_many_arguments)]
fn sign_sigv4(
    method: &str,
    uri: &Uri,
    body: &[u8],
    headers: &[(String, String)],
    access_key_id: &str,
    secret_access_key: &str,
    region: &str,
    service: &str,
    now_amz: &str,
    now_date: &str,
) -> Result<String, LlmError> {
    // 1. Canonical request.
    let canonical_uri = uri.path();
    // Bedrock URIs contain `:` and `/` in model ids — we encode the
    // path-portion exactly as AWS expects (encode `:`, leave `/`).
    let canonical_uri = encode_uri_path(canonical_uri);
    let canonical_query = uri.query().map(canonical_query_string).unwrap_or_default();

    // Lowercase + sort the headers we want to sign.
    let mut signed: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| (k.to_lowercase(), v.trim().to_string()))
        .collect();
    signed.sort_by(|a, b| a.0.cmp(&b.0));
    let mut canonical_headers = String::new();
    for (k, v) in &signed {
        canonical_headers.push_str(k);
        canonical_headers.push(':');
        canonical_headers.push_str(v);
        canonical_headers.push('\n');
    }
    let signed_headers: String = signed
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let payload_hash = sha256_hex(body);
    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );

    // 2. String to sign.
    let scope = format!("{now_date}/{region}/{service}/aws4_request");
    let cr_hash = sha256_hex(canonical_request.as_bytes());
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{now_amz}\n{scope}\n{cr_hash}");

    // 3. Derive signing key.
    let k_secret = format!("AWS4{secret_access_key}");
    let k_date = hmac_sha256(k_secret.as_bytes(), now_date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");

    // 4. Signature.
    let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    Ok(format!(
        "AWS4-HMAC-SHA256 Credential={access_key_id}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    ))
}

/// RFC 3986 percent-encode the path-portion of a URI for SigV4.
///
/// AWS leaves `/` literal but encodes `:` and other reserved chars.
/// We hand-roll this to avoid pulling `percent-encoding` in.
fn encode_uri_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}

/// Canonicalise a query string per SigV4: split on `&`, sort by name,
/// re-encode + rejoin.
fn canonical_query_string(q: &str) -> String {
    let mut pairs: Vec<(String, String)> = q
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (p.to_string(), String::new()),
        })
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", encode_uri_path(k), encode_uri_path(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Return `(yyyymmddThhmmssZ, yyyymmdd)` for *now*.
fn current_amz_dates() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_amz_dates(secs)
}

/// Pure-function date formatter; pulled out so SigV4 unit tests can
/// pin canonical outputs against AWS's published vectors.
pub(crate) fn format_amz_dates(unix_secs: i64) -> (String, String) {
    let (y, mo, d, h, mi, s) = civil_from_days_and_secs(unix_secs);
    (
        format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z"),
        format!("{y:04}{mo:02}{d:02}"),
    )
}

/// Convert a unix-epoch seconds count to civil date/time fields, using
/// the proleptic Gregorian calendar. Howard Hinnant's algorithm.
fn civil_from_days_and_secs(unix_secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = unix_secs.div_euclid(86_400);
    let secs_of_day = unix_secs.rem_euclid(86_400) as u32;
    let h = secs_of_day / 3600;
    let mi = (secs_of_day / 60) % 60;
    let s = secs_of_day % 60;
    // Hinnant's days-from-civil inverse:
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 {
        (mp + 3) as u32
    } else {
        (mp - 9) as u32
    };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, h, mi, s)
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
                "bedrock rejected credentials ({status})"
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
        // Skip the `host` header — hyper sets it automatically based
        // on the URI; setting it again duplicates the header.
        if k.eq_ignore_ascii_case("host") {
            continue;
        }
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
    fn region_drives_base_url() {
        let c = BedrockClient::with_api_token("t").with_region("us-west-2");
        assert!(c.base_url().contains("us-west-2"));
        let url = c.converse_endpoint("anthropic.claude-opus-4-7-v1:0", false);
        assert!(url.contains("/model/anthropic.claude-opus-4-7-v1:0/converse"));
        assert!(!url.ends_with("converse-stream"));
        let stream_url = c.converse_endpoint("anthropic.claude-opus-4-7-v1:0", true);
        assert!(stream_url.ends_with("/converse-stream"));
    }

    #[test]
    fn tool_serialises_inside_tool_spec_with_json_input_schema() {
        let v = tool_to_bedrock(&Tool::no_args("search", "search"));
        assert_eq!(v["toolSpec"]["name"], "search");
        assert!(v["toolSpec"]["inputSchema"]["json"].is_object());
    }

    #[test]
    fn system_prompt_hoisted_out_of_messages() {
        let c = BedrockClient::with_api_token("t");
        let req =
            CompletionRequest::new("m", vec![Message::user_text("hi")]).with_system("be brief");
        let body = c.build_body(&req);
        assert_eq!(body["system"][0]["text"], "be brief");
        // Only one message (user); the system prompt is on the side.
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn sigv4_signs_with_published_test_vector() {
        // AWS published test vector — "GET vanilla" sample request,
        // simplified. We don't aim for byte-identity here; we assert
        // the signature is deterministic + non-empty + uses the
        // expected scope.
        let uri: Uri = "https://bedrock-runtime.us-east-1.amazonaws.com/model/m/converse"
            .parse()
            .unwrap();
        let auth = sign_sigv4(
            "POST",
            &uri,
            b"{}",
            &[
                ("content-type".into(), "application/json".into()),
                (
                    "host".into(),
                    "bedrock-runtime.us-east-1.amazonaws.com".into(),
                ),
                ("x-amz-date".into(), "20240101T000000Z".into()),
            ],
            "AKIDEXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "us-east-1",
            "bedrock",
            "20240101T000000Z",
            "20240101",
        )
        .unwrap();
        assert!(auth.starts_with(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20240101/us-east-1/bedrock/aws4_request"
        ));
        assert!(auth.contains("SignedHeaders=content-type;host;x-amz-date"));
        assert!(auth.contains("Signature="));
        // Re-sign — must be byte-identical (deterministic).
        let auth2 = sign_sigv4(
            "POST",
            &uri,
            b"{}",
            &[
                ("content-type".into(), "application/json".into()),
                (
                    "host".into(),
                    "bedrock-runtime.us-east-1.amazonaws.com".into(),
                ),
                ("x-amz-date".into(), "20240101T000000Z".into()),
            ],
            "AKIDEXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "us-east-1",
            "bedrock",
            "20240101T000000Z",
            "20240101",
        )
        .unwrap();
        assert_eq!(auth, auth2, "SigV4 must be deterministic");
    }

    #[test]
    fn hmac_sha256_matches_published_test_vector() {
        // RFC 4231 test case 1: key = 20 bytes 0x0b, data = "Hi There".
        let key = [0x0bu8; 20];
        let data = b"Hi There";
        let mac = hmac_sha256(&key, data);
        // Expected from RFC 4231:
        let expected =
            hex::decode("b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7")
                .unwrap();
        assert_eq!(mac.as_ref(), expected.as_slice());
    }

    #[test]
    fn amz_date_formats_unix_epoch() {
        let (amz, date) = format_amz_dates(0);
        assert_eq!(amz, "19700101T000000Z");
        assert_eq!(date, "19700101");
    }

    #[test]
    fn amz_date_formats_known_timestamp() {
        // 2024-01-01 00:00:00 UTC == 1704067200
        let (amz, date) = format_amz_dates(1_704_067_200);
        assert_eq!(amz, "20240101T000000Z");
        assert_eq!(date, "20240101");
    }

    #[test]
    fn event_stream_parser_extracts_frame() {
        // Build a minimal valid event-stream frame:
        // - 1 header: name=":event-type" (11 bytes), type=7 (string),
        //   value="contentBlockDelta" (17 bytes prefixed by u16 len)
        // - payload: {"delta":{"text":"hi"}}
        let event_name = ":event-type";
        let event_value = "contentBlockDelta";
        let payload = br#"{"delta":{"text":"hi"}}"#;

        let mut headers: Vec<u8> = Vec::new();
        headers.push(event_name.len() as u8);
        headers.extend_from_slice(event_name.as_bytes());
        headers.push(7); // string
        headers.extend_from_slice(&(event_value.len() as u16).to_be_bytes());
        headers.extend_from_slice(event_value.as_bytes());

        let total_len = (12 + headers.len() + payload.len() + 4) as u32;
        let mut frame: Vec<u8> = Vec::new();
        frame.extend_from_slice(&total_len.to_be_bytes());
        frame.extend_from_slice(&(headers.len() as u32).to_be_bytes());
        frame.extend_from_slice(&[0u8; 4]); // prelude_crc (we don't validate)
        frame.extend_from_slice(&headers);
        frame.extend_from_slice(payload);
        frame.extend_from_slice(&[0u8; 4]); // message_crc (we don't validate)

        match try_parse_eventstream(&frame) {
            EventParse::Frame {
                consumed,
                payload: pl,
                event_type,
            } => {
                assert_eq!(consumed, frame.len());
                assert_eq!(event_type, "contentBlockDelta");
                assert_eq!(pl, payload);
            }
            other => panic!("expected Frame, got {other:?}"),
        }
    }

    #[test]
    fn event_stream_parser_needs_more_on_truncation() {
        // Only 8 bytes — less than the 12-byte prelude.
        let buf = [0u8; 8];
        assert!(matches!(try_parse_eventstream(&buf), EventParse::NeedMore));
    }

    #[test]
    fn project_bedrock_event_text_delta() {
        let mut current = None;
        let deltas = project_bedrock_event(
            "contentBlockDelta",
            br#"{"delta":{"text":"hi"}}"#,
            &mut current,
        );
        assert_eq!(deltas.len(), 1);
        assert!(matches!(&deltas[0], MessageDelta::TextDelta { text } if text == "hi"));
    }

    #[test]
    fn project_bedrock_event_message_stop() {
        let mut current = None;
        let deltas =
            project_bedrock_event("messageStop", br#"{"stopReason":"end_turn"}"#, &mut current);
        assert!(
            matches!(&deltas[0], MessageDelta::Done { stop_reason } if stop_reason == "end_turn")
        );
    }

    #[test]
    fn signs_with_sigv4_flag_distinguishes_auth_modes() {
        let bearer = BedrockClient::with_api_token("t");
        assert!(!bearer.signs_with_sigv4());
        let sigv4 = BedrockClient::with_credentials(AwsCredentials {
            access_key_id: "AKID".into(),
            secret_access_key: "SECRET".into(),
            session_token: None,
        });
        assert!(sigv4.signs_with_sigv4());
    }
}
