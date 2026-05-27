//! OpenAI Responses API client — v0.26 **skeleton**.
//!
//! Auth + request-shaping + endpoint routing are in. The actual
//! response-parse + streaming-SSE conversion is `TODO v0.27` so we
//! return a stub [`Message::assistant_text`] from `complete()` to
//! satisfy the trait surface. This lets the typed shape land + lets
//! Track B's `@tool` macro hand serialisations to *all four*
//! providers without conditionally compiling them out.
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
//! [`model_endpoint`] helper is wired for future fine-tuned-model
//! routing (`organization`-scoped models live on a different sub-path).

use crate::llm::budget::{DollarBudget, TokenBudget};
use crate::llm::error::LlmError;
use crate::llm::message::Message;
use crate::llm::provider::{CompletionRequest, LlmProvider};
use crate::llm::streaming::MessageStream;
use crate::llm::tools::Tool;

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

    /// Map a model name to its endpoint sub-path. v0.26 ships one
    /// endpoint for every model; v0.27 will route fine-tuned models
    /// under `/v1/organizations/.../responses`.
    fn model_endpoint(&self, _model: &str) -> String {
        format!("{}/v1/responses", self.base_url.trim_end_matches('/'))
    }

    /// v0.26: build the request body but don't ship it. Pulled out
    /// so we can assert it shapes correctly in unit tests without
    /// hitting the network.
    pub fn build_body(&self, req: &CompletionRequest) -> serde_json::Value {
        let mut input: Vec<serde_json::Value> = Vec::new();
        if let Some(sys) = &req.system {
            input.push(serde_json::json!({
                "role": "developer",
                "content": [{"type": "input_text", "text": sys}],
            }));
        }
        for m in &req.messages {
            let role = match m.role {
                crate::llm::message::Role::User | crate::llm::message::Role::Tool => "user",
                crate::llm::message::Role::Assistant => "assistant",
                crate::llm::message::Role::System => "developer",
            };
            input.push(serde_json::json!({
                "role": role,
                "content": m.content,
            }));
        }
        let mut body = serde_json::json!({
            "model": req.model,
            "input": input,
        });
        if !req.tools.is_empty() {
            body["tools"] =
                serde_json::Value::Array(req.tools.iter().map(tool_to_openai).collect());
        }
        if let Some(t) = req.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(n) = req.max_tokens {
            body["max_output_tokens"] = serde_json::json!(n);
        }
        body
    }

    /// Pre-deduct the typed budgets so the skeleton path still
    /// surfaces `BudgetExhausted` for round-trip tests.
    fn account_stub_usage(
        req: &CompletionRequest,
        budget_token: Option<&TokenBudget>,
        budget_dollar: Option<&DollarBudget>,
    ) -> Result<(), LlmError> {
        if let Some(b) = budget_token {
            if b.is_exhausted() {
                return Err(LlmError::BudgetExhausted(
                    crate::llm::error::BudgetExhausted::tokens(b.limit(), b.consumed()),
                ));
            }
        }
        if let Some(b) = budget_dollar {
            if b.is_exhausted() {
                return Err(LlmError::BudgetExhausted(
                    crate::llm::error::BudgetExhausted::dollars(
                        b.limit_cents(),
                        b.consumed_cents(),
                    ),
                ));
            }
        }
        let _ = req;
        Ok(())
    }
}

fn tool_to_openai(t: &Tool) -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "name": t.name,
        "description": t.description,
        "parameters": t.input_schema,
    })
}

#[async_trait::async_trait]
impl LlmProvider for OpenAiClient {
    async fn complete(&self, req: CompletionRequest) -> Result<Message, LlmError> {
        Self::account_stub_usage(&req, req.token_budget.as_ref(), req.dollar_budget.as_ref())?;
        // Auth gate — surface a clear error if no key is set so a
        // mistaken `OpenAiClient::from_env()` doesn't silently
        // succeed on the stub path.
        if self.api_key.is_empty() {
            return Err(LlmError::Auth("OpenAI api key empty".into()));
        }
        // Shape the request body to fail fast on bad inputs, but
        // don't send it — v0.27 will wire the actual POST.
        let _body = self.build_body(&req);
        let _endpoint = self.model_endpoint(&req.model);
        // Stub return so the trait surface compiles and downstream
        // structural tests can pin the shape.
        Ok(Message::assistant_text(format!(
            "[openai stub v0.26 — model={} would send to {_endpoint}]",
            req.model
        )))
    }

    async fn complete_stream(&self, req: CompletionRequest) -> Result<MessageStream, LlmError> {
        // v0.27 will wire SSE. Today: yield the same stub text as a
        // single TextDelta + a Done event so streaming callers can
        // exercise their wiring.
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
        tool_to_openai(tool)
    }
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
    }

    #[tokio::test]
    async fn stub_complete_returns_v0_26_marker_text() {
        let c = OpenAiClient::with_api_key("k");
        let m = c
            .complete(CompletionRequest::new("gpt-5", vec![]))
            .await
            .unwrap();
        assert!(m.text().contains("openai stub v0.26"));
    }
}
