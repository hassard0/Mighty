//! Google Gemini `generateContent` client — v0.26 **skeleton**.
//!
//! Same shape as the other v0.26 skeletons: auth + endpoint routing
//! land; full body/response parsing is `TODO v0.27`.
//!
//! ## Auth
//!
//! `GEMINI_API_KEY` env var (or `GOOGLE_API_KEY` — checked as a
//! fallback). The key goes on the URL as `?key=<key>` per Google's
//! convention; the rest of the request body matches
//! `application/json`.
//!
//! ## Endpoint
//!
//! `POST https://generativelanguage.googleapis.com/v1beta/models/<MODEL>:generateContent`.
//! Streaming uses the `streamGenerateContent` variant.

use crate::llm::error::LlmError;
use crate::llm::message::Message;
use crate::llm::provider::{CompletionRequest, LlmProvider};
use crate::llm::streaming::MessageStream;
use crate::llm::tools::Tool;

#[derive(Debug, Clone)]
pub struct GeminiClient {
    api_key: String,
    base_url: String,
}

impl GeminiClient {
    pub fn from_env() -> Result<Self, LlmError> {
        let key = std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .map_err(|_| LlmError::Auth("GEMINI_API_KEY / GOOGLE_API_KEY not set".into()))?;
        Ok(Self::with_api_key(key))
    }

    pub fn with_api_key(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://generativelanguage.googleapis.com".into(),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Map a model name + `(stream?)` to the `:generateContent` or
    /// `:streamGenerateContent` URL.
    pub fn model_endpoint(&self, model: &str, stream: bool) -> String {
        let verb = if stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        format!(
            "{}/v1beta/models/{}:{}?key={}",
            self.base_url.trim_end_matches('/'),
            model,
            verb,
            self.api_key
        )
    }

    /// Build the request body. v0.26: structurally correct; v0.27
    /// will fill in tool-use shaping + `safetySettings`.
    pub fn build_body(&self, req: &CompletionRequest) -> serde_json::Value {
        let contents: Vec<serde_json::Value> = req
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    crate::llm::message::Role::Assistant => "model",
                    _ => "user",
                };
                let parts: Vec<serde_json::Value> = m
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        crate::llm::message::ContentBlock::Text { text } => {
                            Some(serde_json::json!({ "text": text }))
                        }
                        _ => None,
                    })
                    .collect();
                serde_json::json!({ "role": role, "parts": parts })
            })
            .collect();
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
        body
    }
}

fn tool_to_gemini(t: &Tool) -> serde_json::Value {
    serde_json::json!({
        "name": t.name,
        "description": t.description,
        "parameters": t.input_schema,
    })
}

#[async_trait::async_trait]
impl LlmProvider for GeminiClient {
    async fn complete(&self, req: CompletionRequest) -> Result<Message, LlmError> {
        if self.api_key.is_empty() {
            return Err(LlmError::Auth("Gemini api key empty".into()));
        }
        let _body = self.build_body(&req);
        let _endpoint = self.model_endpoint(&req.model, false);
        Ok(Message::assistant_text(format!(
            "[gemini stub v0.26 — model={}]",
            req.model
        )))
    }

    async fn complete_stream(&self, req: CompletionRequest) -> Result<MessageStream, LlmError> {
        let msg = self.complete(req).await?;
        let text = msg.text();
        Ok(MessageStream::from_vec(vec![
            Ok(crate::llm::message::MessageDelta::TextDelta { text }),
            Ok(crate::llm::message::MessageDelta::Done {
                stop_reason: "STOP".into(),
            }),
        ]))
    }

    fn schema_for_tool(&self, tool: &Tool) -> serde_json::Value {
        tool_to_gemini(tool)
    }
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

    #[tokio::test]
    async fn stub_returns_v0_26_marker() {
        let c = GeminiClient::with_api_key("k");
        let m = c
            .complete(CompletionRequest::new("gemini-2.5-pro", vec![]))
            .await
            .unwrap();
        assert!(m.text().contains("gemini stub v0.26"));
    }
}
