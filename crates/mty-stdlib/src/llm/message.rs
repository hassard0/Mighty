//! Typed message + content shapes for `std.llm`.
//!
//! Designed against the Anthropic Messages API content model because
//! it's the strictly-typed superset of OpenAI / Gemini / Bedrock:
//!
//! - Roles are an enum (`User`, `Assistant`, `System`, `Tool`) — Gemini
//!   uses `model` instead of `assistant`; we normalise on the way in
//!   and out.
//! - Content is a *list* of typed blocks per message, never a flat
//!   string. Text + tool-use + tool-result + image are all first-class
//!   so a single round-trip can carry parallel tool-uses without the
//!   provider-specific "function_call" string-encoding dance.
//! - Tool-use IDs are opaque strings (Anthropic uses `toolu_*`, OpenAI
//!   uses `call_*`); the typed shape carries whatever the provider
//!   issued so the next user-turn can pair `tool_use` with
//!   `tool_result` correctly.
//!
//! See `docs/reference/stdlib/llm.md` for the Mighty-side surface.

use serde::{Deserialize, Serialize};

/// Who said it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    /// Top-of-conversation instructions. Anthropic carries `system`
    /// as a top-level field rather than a message; we still surface
    /// it in [`Message`] so the conversion lives in one place.
    System,
    /// Synthetic role for tool result messages going *back* to the
    /// model. Anthropic encodes these as `user` messages with a
    /// `tool_result` content block; OpenAI uses a literal `tool`
    /// role. We normalise to the typed enum and re-serialise per
    /// provider in `anthropic.rs` / `openai.rs`.
    Tool,
}

impl Role {
    pub fn as_anthropic(self) -> &'static str {
        match self {
            Role::User | Role::Tool => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
        }
    }
}

/// A single message in a conversation. `content` is *always* a list of
/// blocks; collapsing to a single string when serialising to providers
/// that prefer that shape (Gemini text-only) happens at the provider
/// boundary, not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    /// Convenience: build a user message from a single text string.
    pub fn user_text(s: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::text(s)],
        }
    }

    /// Convenience: build an assistant message from a single text
    /// string. Used by the streaming aggregator + by mock helpers.
    pub fn assistant_text(s: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::text(s)],
        }
    }

    /// Convenience: gather all text-block content into one `String`,
    /// joined by `"\n"`. Returns empty string if the message carries
    /// no text blocks. Useful for `let reply.text(): Str`-style call
    /// sites from Mighty.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (i, b) in self.content.iter().enumerate() {
            if let ContentBlock::Text { text } = b {
                if i > 0 && !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            }
        }
        out
    }

    /// Extract all tool-use blocks. Callers run each one and feed the
    /// results back as a follow-up `Tool`-role message.
    pub fn tool_uses(&self) -> Vec<&ToolUse> {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse(tu) => Some(tu),
                _ => None,
            })
            .collect()
    }
}

/// One block inside a [`Message::content`] list.
///
/// We mirror Anthropic's discriminator-tagged shape (`type: "text"`,
/// `type: "tool_use"`, `type: "tool_result"`, `type: "image"`) so
/// `serde_json::to_value(&block)` already gives us the right body for
/// `POST /v1/messages`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    /// The assistant decided to call a tool. `id` pairs this with
    /// the corresponding `ToolResult` on the next user turn.
    ToolUse(ToolUse),
    /// The user turn returns a tool's output to the model.
    ToolResult(ToolResult),
    /// Image input — `source` carries the provider-specific source
    /// shape (`{ type: "base64", media_type, data }` for Anthropic).
    Image {
        source: ImageSource,
    },
}

impl ContentBlock {
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text { text: s.into() }
    }
}

/// The assistant's tool-call payload. `input` is the
/// already-deserialised arguments object — providers all encode tool
/// arguments as JSON so we keep them as `serde_json::Value` rather
/// than re-typing per tool here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUse {
    /// Opaque id from the provider (Anthropic `toolu_*`, OpenAI
    /// `call_*`). Pair with [`ToolResult::tool_use_id`].
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// A tool's output going back to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_use_id: String,
    /// Free-form content — usually a JSON-stringified result. Anthropic
    /// also accepts a list of content blocks here; we keep it flat for
    /// the v0.26 surface.
    pub content: String,
    /// Whether the tool itself errored. Anthropic shows this as
    /// `is_error: true` on the result block.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_error: bool,
}

// `skip_serializing_if` on serde requires a `&T -> bool` function;
// the trivially-copy clippy lint complains but the signature is
// dictated by serde.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !b
}

/// Image source for [`ContentBlock::Image`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

/// One incremental piece of an [`Message`] from a streaming
/// completion. Emitted by [`crate::llm::streaming::MessageStream`].
///
/// Three event kinds today:
///
/// - `TextDelta { text }` — append `text` to the open text block.
/// - `ToolUseDelta { id, name, input_partial }` — accumulate tool-use
///   input fragments. Providers fragment the JSON; we stitch.
/// - `Done` — terminal event. After this the stream yields `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageDelta {
    TextDelta {
        text: String,
    },
    ToolUseDelta {
        id: String,
        name: String,
        input_partial: String,
    },
    /// Indicates the assistant is finished. `stop_reason` mirrors the
    /// provider's value (`end_turn`, `tool_use`, `max_tokens`, ...).
    Done {
        stop_reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_serialises_to_anthropic_user_for_tool() {
        // Tool-role messages serialise as `user` on the Anthropic wire.
        assert_eq!(Role::Tool.as_anthropic(), "user");
        assert_eq!(Role::Assistant.as_anthropic(), "assistant");
    }

    #[test]
    fn text_block_round_trips_through_serde() {
        let m = Message::user_text("hi");
        let j = serde_json::to_value(&m).unwrap();
        assert_eq!(j["role"], "user");
        assert_eq!(j["content"][0]["type"], "text");
        assert_eq!(j["content"][0]["text"], "hi");
    }

    #[test]
    fn tool_use_block_serialises_with_discriminator() {
        let block = ContentBlock::ToolUse(ToolUse {
            id: "toolu_01".into(),
            name: "search".into(),
            input: serde_json::json!({ "q": "rust" }),
        });
        let j = serde_json::to_value(&block).unwrap();
        assert_eq!(j["type"], "tool_use");
        assert_eq!(j["id"], "toolu_01");
        assert_eq!(j["input"]["q"], "rust");
    }

    #[test]
    fn message_text_gathers_all_text_blocks() {
        let m = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::text("hello"),
                ContentBlock::ToolUse(ToolUse {
                    id: "x".into(),
                    name: "y".into(),
                    input: serde_json::Value::Null,
                }),
                ContentBlock::text("world"),
            ],
        };
        assert_eq!(m.text(), "hello\nworld");
        assert_eq!(m.tool_uses().len(), 1);
    }
}
