//! Tiny Anthropic Messages client (tool-use enabled).
//!
//! The harness is intentionally **not** built on top of
//! `mty-stdlib::llm` — that crate has its own retry / streaming /
//! budget plumbing we don't need here, and pulling it in would
//! drag the whole Mighty workspace into the bench build.
//!
//! Surface: one `Client::messages()` call that takes a tool list +
//! a running conversation and returns either a text reply or a
//! list of `tool_use` blocks the agent must execute and reply to.
//!
//! Cost accounting: every response reports `input_tokens` and
//! `output_tokens`; we multiply by per-million-token rates from
//! `model_pricing()` to get an estimated USD cost.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

pub struct Client {
    api_key: String,
    http: reqwest::Client,
}

impl Client {
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
            anyhow!(
                "ANTHROPIC_API_KEY required for smoke run. Set it and retry, or use 'make bench-full' (gated)."
            )
        })?;
        let http = reqwest::Client::builder()
            .user_agent("mty-swe-bench/0.1")
            .timeout(std::time::Duration::from_secs(120))
            .build()?;
        Ok(Self { api_key, http })
    }

    pub async fn messages(
        &self,
        model: &str,
        system: &str,
        history: &[Message],
        tools: &[Tool],
        max_tokens: u32,
    ) -> Result<Response> {
        let body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "system": system,
            "tools": tools,
            "messages": history,
        });
        let resp = self
            .http
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("anthropic POST failed")?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            return Err(anyhow!(
                "anthropic returned {}: {}",
                status,
                String::from_utf8_lossy(&bytes)
            ));
        }
        let parsed: Response = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "parse anthropic response: {}",
                String::from_utf8_lossy(&bytes)
            )
        })?;
        Ok(parsed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Response {
    /// Anthropic-assigned message ID (kept for log correlation).
    #[allow(dead_code)]
    pub id: String,
    /// Echo of the model field — same as the request unless Anthropic
    /// up/down-routes (which it doesn't today, but we capture it).
    #[allow(dead_code)]
    pub model: String,
    pub stop_reason: Option<String>,
    pub content: Vec<ContentBlock>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Per-million-token pricing in USD, as published by Anthropic.
/// Bump these when Anthropic changes rates — see
/// https://www.anthropic.com/pricing.
pub fn model_pricing(model: &str) -> (f64, f64) {
    // (input_per_million, output_per_million)
    if model.contains("opus") {
        // Opus 4.7 and prior 4.x all bill at the same headline rate.
        (15.0, 75.0)
    } else if model.contains("sonnet") {
        (3.0, 15.0)
    } else if model.contains("haiku") {
        (0.8, 4.0)
    } else {
        // Unknown — be pessimistic so the budget cap fires early.
        (15.0, 75.0)
    }
}

pub fn cost_usd(model: &str, usage: &Usage) -> f64 {
    let (in_rate, out_rate) = model_pricing(model);
    let in_cost = (usage.input_tokens as f64) * in_rate / 1_000_000.0;
    let out_cost = (usage.output_tokens as f64) * out_rate / 1_000_000.0;
    in_cost + out_cost
}
