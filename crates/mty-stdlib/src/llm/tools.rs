//! Tool definitions for `std.llm`.
//!
//! A [`Tool`] is *what the model is allowed to call*. The runtime
//! representation Track B's `@tool` macro emits at compile time is
//! exactly this struct — the macro just synthesises the JSON-schema
//! `input_schema` from the Rust/Mighty function signature, sets the
//! `handler` closure, and registers it in the user's `tools: [...]`
//! list.
//!
//! Provider serialisation happens at the [`crate::llm::LlmProvider`]
//! boundary — Anthropic wants
//! `{ name, description, input_schema: { type: "object", ... } }`,
//! OpenAI wraps the same shape in `{ type: "function", function: {...} }`.
//! Both transforms live in their respective provider modules
//! (`anthropic.rs`, `openai.rs`) so this struct stays surface-agnostic.
//!
//! `ToolUse` and `ToolResult` live in [`crate::llm::message`] because
//! they're per-turn content blocks, not per-conversation tool defs.
//! Re-exported here for the import convenience of the `@tool` macro.

use serde::{Deserialize, Serialize};

pub use crate::llm::message::{ToolResult, ToolUse};

/// A tool the model is allowed to call.
///
/// `input_schema` is the JSON Schema fragment for the tool's
/// parameters object. The conventional shape is:
///
/// ```json
/// {
///   "type": "object",
///   "properties": { "<arg>": { "type": "string", "description": "..." } },
///   "required": ["<arg>"]
///  }
/// ```
///
/// We don't validate it eagerly — providers will reject malformed
/// schemas with a clear 400; revalidating client-side just doubles
/// the failure surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

impl Tool {
    /// Convenience constructor — most callers go through the `@tool`
    /// macro instead. Useful for hand-built tools + tests.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: serde_json::Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }

    /// Empty tool — `name + description` only, no parameters. Useful
    /// for sentinel tools like a "stop" signal that takes no args.
    pub fn no_args(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self::new(
            name,
            description,
            serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        )
    }
}

/// Hint to the provider about *how* tools may be invoked on this
/// turn. Most callers leave this `Auto` (the model picks); other
/// modes are for tightly-orchestrated agent loops where you know the
/// next step.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolChoice {
    /// The model decides whether to call a tool. Provider default.
    #[default]
    Auto,
    /// The model *must* emit a tool call this turn.
    Any,
    /// The model must call this specific tool.
    Tool { name: String },
    /// Tools are off this turn — model returns a plain text reply.
    None,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_args_tool_has_empty_object_schema() {
        let t = Tool::no_args("stop", "halt");
        assert_eq!(t.input_schema["type"], "object");
        assert_eq!(t.input_schema["properties"].as_object().unwrap().len(), 0);
    }

    #[test]
    fn tool_choice_serialises_with_type_discriminator() {
        let c = ToolChoice::Tool {
            name: "search".into(),
        };
        let j = serde_json::to_value(&c).unwrap();
        assert_eq!(j["type"], "tool");
        assert_eq!(j["name"], "search");

        let auto = ToolChoice::Auto;
        let j = serde_json::to_value(&auto).unwrap();
        assert_eq!(j["type"], "auto");
    }
}
