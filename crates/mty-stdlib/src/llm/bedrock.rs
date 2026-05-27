//! AWS Bedrock Converse API client — v0.26 **skeleton**.
//!
//! Auth + region routing + request shape land. SigV4 signing is
//! `TODO v0.27` — the v0.26 skeleton authenticates via the
//! `AWS_BEDROCK_API_TOKEN` env (which Bedrock now supports for
//! short-lived API tokens, sidestepping SigV4 for the v0.26 surface).
//! For full IAM-credential SigV4 callers, v0.27 wires `aws-sigv4` in.
//!
//! ## Endpoint
//!
//! `POST https://bedrock-runtime.<REGION>.amazonaws.com/model/<MODEL_ID>/converse`.
//! Region defaults to `us-east-1`; override via [`BedrockClient::with_region`].

use crate::llm::error::LlmError;
use crate::llm::message::Message;
use crate::llm::provider::{CompletionRequest, LlmProvider};
use crate::llm::streaming::MessageStream;
use crate::llm::tools::Tool;

#[derive(Debug, Clone)]
pub struct BedrockClient {
    /// Short-lived bearer token (v0.26). v0.27: SigV4 with
    /// `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`.
    api_token: String,
    region: String,
    base_url_override: Option<String>,
}

impl BedrockClient {
    pub fn from_env() -> Result<Self, LlmError> {
        let token = std::env::var("AWS_BEDROCK_API_TOKEN")
            .map_err(|_| LlmError::Auth("AWS_BEDROCK_API_TOKEN not set".into()))?;
        let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".into());
        Ok(Self {
            api_token: token,
            region,
            base_url_override: None,
        })
    }

    pub fn with_api_token(token: impl Into<String>) -> Self {
        Self {
            api_token: token.into(),
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

    fn base_url(&self) -> String {
        self.base_url_override
            .clone()
            .unwrap_or_else(|| format!("https://bedrock-runtime.{}.amazonaws.com", self.region))
    }

    /// Endpoint for the Converse API. Model id is URL-encoded
    /// minimally — Bedrock's ids contain `:` and `/` which we leave
    /// as-is per AWS's documented URL shape.
    pub fn converse_endpoint(&self, model_id: &str) -> String {
        format!("{}/model/{}/converse", self.base_url(), model_id)
    }

    pub fn build_body(&self, req: &CompletionRequest) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = req
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    crate::llm::message::Role::Assistant => "assistant",
                    _ => "user",
                };
                let content: Vec<serde_json::Value> = m
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        crate::llm::message::ContentBlock::Text { text } => {
                            Some(serde_json::json!({ "text": text }))
                        }
                        _ => None,
                    })
                    .collect();
                serde_json::json!({ "role": role, "content": content })
            })
            .collect();
        let mut body = serde_json::json!({ "messages": messages });
        if let Some(sys) = &req.system {
            body["system"] = serde_json::json!([{ "text": sys }]);
        }
        if !req.tools.is_empty() {
            body["toolConfig"] = serde_json::json!({
                "tools": req.tools.iter().map(tool_to_bedrock).collect::<Vec<_>>()
            });
        }
        if let Some(t) = req.temperature {
            body["inferenceConfig"] = serde_json::json!({ "temperature": t });
        }
        body
    }
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

#[async_trait::async_trait]
impl LlmProvider for BedrockClient {
    async fn complete(&self, req: CompletionRequest) -> Result<Message, LlmError> {
        if self.api_token.is_empty() {
            return Err(LlmError::Auth("Bedrock api token empty".into()));
        }
        let _body = self.build_body(&req);
        let _endpoint = self.converse_endpoint(&req.model);
        Ok(Message::assistant_text(format!(
            "[bedrock stub v0.26 — model={} region={}]",
            req.model, self.region
        )))
    }

    async fn complete_stream(&self, req: CompletionRequest) -> Result<MessageStream, LlmError> {
        let msg = self.complete(req).await?;
        let text = msg.text();
        Ok(MessageStream::from_vec(vec![
            Ok(crate::llm::message::MessageDelta::TextDelta { text }),
            Ok(crate::llm::message::MessageDelta::Done {
                stop_reason: "end_turn".into(),
            }),
        ]))
    }

    fn schema_for_tool(&self, tool: &Tool) -> serde_json::Value {
        tool_to_bedrock(tool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_drives_base_url() {
        let c = BedrockClient::with_api_token("t").with_region("us-west-2");
        assert!(c.base_url().contains("us-west-2"));
        let url = c.converse_endpoint("anthropic.claude-opus-4-7-v1:0");
        assert!(url.contains("/model/anthropic.claude-opus-4-7-v1:0/converse"));
    }

    #[test]
    fn tool_serialises_inside_tool_spec_with_json_input_schema() {
        let v = tool_to_bedrock(&Tool::no_args("search", "search"));
        assert_eq!(v["toolSpec"]["name"], "search");
        assert!(v["toolSpec"]["inputSchema"]["json"].is_object());
    }

    #[tokio::test]
    async fn stub_returns_v0_26_marker() {
        let c = BedrockClient::with_api_token("t");
        let m = c
            .complete(CompletionRequest::new(
                "anthropic.claude-opus-4-7-v1:0",
                vec![],
            ))
            .await
            .unwrap();
        assert!(m.text().contains("bedrock stub v0.26"));
    }
}
