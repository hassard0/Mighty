//! `std.memory.Working` — bounded scratchpad for "what I'm thinking
//! right now" between LLM calls in a single agent turn.
//!
//! The scratchpad is a labelled list of strings; the
//! [`Working::render`] method produces a Markdown summary suitable
//! for splicing into the next prompt:
//!
//! ```text
//! ## Working Memory
//! - **plan**: outline the introduction
//! - **note**: user prefers concise output
//! ```
//!
//! The "bounded" half is a soft token budget (default 2,048 tokens
//! ≈ 8,000 characters). When a `push` would exceed the budget the
//! oldest entries are dropped one-by-one until the new entry fits.
//! Token counting is a deliberately-cheap approximation
//! ([`approx_tokens`]) — production code should swap in a real
//! tokenizer via a downstream adapter.

use super::snapshot::{record_memory_delta, MemoryDelta, SnapshotBytes};
use super::MemoryHandle;
use serde::{Deserialize, Serialize};

/// Default token budget when [`Working::new`] is called without an
/// explicit budget. Picked to fit comfortably alongside an Anthropic
/// Claude / OpenAI GPT-class context window minus the system prompt
/// and a generous output reserve.
pub const DEFAULT_TOKEN_BUDGET: usize = 2_048;

/// Rough characters-per-token ratio used by [`approx_tokens`]. The
/// real value depends on the tokenizer + the text shape; 4.0 is a
/// safe upper bound for English prose.
pub const CHARS_PER_TOKEN: f32 = 4.0;

/// One entry on the scratchpad.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkingEntry {
    pub label: String,
    pub content: String,
}

impl WorkingEntry {
    /// Approximate token cost of this entry — label + content +
    /// rendering overhead.
    pub fn token_cost(&self) -> usize {
        approx_tokens(&self.label) + approx_tokens(&self.content) + 4
    }
}

/// Scratchpad with a soft token budget.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Working {
    pub token_budget: usize,
    pub entries: Vec<WorkingEntry>,
    #[serde(default = "default_handle_id")]
    pub handle_id: String,
}

fn default_handle_id() -> String {
    "working.default".into()
}

impl Default for Working {
    fn default() -> Self {
        Self::new()
    }
}

impl Working {
    /// Build a scratchpad with the default [`DEFAULT_TOKEN_BUDGET`].
    pub fn new() -> Self {
        Self::with_budget(DEFAULT_TOKEN_BUDGET)
    }

    /// Build a scratchpad with an explicit budget. A budget of zero
    /// is clamped to 1 so the first push always lands (and is
    /// immediately the only entry).
    pub fn with_budget(tokens: usize) -> Self {
        Self {
            token_budget: tokens.max(1),
            entries: Vec::new(),
            handle_id: default_handle_id(),
        }
    }

    /// Override the logical handle id used by snapshot/restore.
    pub fn with_handle_id(mut self, id: impl Into<String>) -> Self {
        self.handle_id = id.into();
        self
    }

    /// Number of entries currently held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if the scratchpad has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Append `(label, content)`. Evicts oldest entries one-by-one
    /// until the new entry fits inside `token_budget`.
    pub fn push(&mut self, label: &str, content: &str) {
        let entry = WorkingEntry {
            label: label.to_string(),
            content: content.to_string(),
        };
        // Eagerly trim before push so the new entry's cost is counted
        // against the budget.
        let cost = entry.token_cost();
        while !self.entries.is_empty() && self.current_tokens() + cost > self.token_budget {
            self.entries.remove(0);
        }
        self.entries.push(entry.clone());
        record_memory_delta(
            0,
            &MemoryDelta::Patch {
                handle_kind: self.kind().to_string(),
                handle_id: self.handle_id.clone(),
                op: "push".into(),
                bytes: serde_json::to_vec(&entry).unwrap_or_default(),
            },
        );
    }

    /// Drop every entry. The budget is preserved.
    pub fn clear(&mut self) {
        self.entries.clear();
        record_memory_delta(
            0,
            &MemoryDelta::Patch {
                handle_kind: self.kind().to_string(),
                handle_id: self.handle_id.clone(),
                op: "clear".into(),
                bytes: Vec::new(),
            },
        );
    }

    /// Render a Markdown summary suitable for splicing into the next
    /// LLM prompt. Returns an empty string when the scratchpad is
    /// empty so callers can unconditionally concat.
    pub fn render(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }
        let mut out = String::from("## Working Memory\n");
        for e in &self.entries {
            out.push_str(&format!("- **{}**: {}\n", e.label, e.content));
        }
        out
    }

    /// Sum of [`WorkingEntry::token_cost`] across every entry. Cheap.
    pub fn current_tokens(&self) -> usize {
        self.entries.iter().map(|e| e.token_cost()).sum()
    }

    /// Snapshot the scratchpad state into portable bytes.
    pub fn snapshot_bytes(&self) -> SnapshotBytes {
        <Self as MemoryHandle>::snapshot(self)
    }

    /// Restore from a snapshot produced by [`snapshot_bytes`].
    pub fn restore_bytes(&mut self, snapshot: &SnapshotBytes) -> Result<(), String> {
        <Self as MemoryHandle>::restore(self, snapshot)
    }
}

impl MemoryHandle for Working {
    fn kind(&self) -> &'static str {
        "working"
    }

    fn snapshot(&self) -> SnapshotBytes {
        SnapshotBytes::new(serde_json::to_vec(self).unwrap_or_default())
    }

    fn restore(&mut self, snapshot: &SnapshotBytes) -> Result<(), String> {
        let restored: Working = serde_json::from_slice(snapshot.as_slice())
            .map_err(|e| format!("working snapshot decode: {e}"))?;
        *self = restored;
        Ok(())
    }
}

/// Cheap approximation: `chars().count() / CHARS_PER_TOKEN`, rounded
/// up. Real tokenizers should swap this for their own routine.
pub fn approx_tokens(s: &str) -> usize {
    let chars = s.chars().count();
    if chars == 0 {
        return 0;
    }
    (((chars as f32) / CHARS_PER_TOKEN).ceil()) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_is_markdown_shape() {
        let mut w = Working::new();
        w.push("plan", "outline");
        w.push("note", "concise");
        let rendered = w.render();
        assert!(rendered.starts_with("## Working Memory\n"));
        assert!(rendered.contains("**plan**: outline"));
        assert!(rendered.contains("**note**: concise"));
    }

    #[test]
    fn empty_renders_empty_string() {
        let w = Working::new();
        assert_eq!(w.render(), "");
    }

    #[test]
    fn budget_drops_oldest() {
        // Tiny budget so two non-trivial entries can't co-exist.
        let mut w = Working::with_budget(8);
        w.push("a", "first entry with content");
        w.push("b", "second entry with content");
        // Only the latest survives.
        assert_eq!(w.len(), 1);
        assert_eq!(w.entries[0].label, "b");
    }

    #[test]
    fn clear_empties_keeps_budget() {
        let mut w = Working::with_budget(100);
        w.push("x", "y");
        w.clear();
        assert!(w.is_empty());
        assert_eq!(w.token_budget, 100);
    }

    #[test]
    fn snapshot_round_trip() {
        let mut w = Working::with_budget(64);
        w.push("k", "v");
        let snap = w.snapshot_bytes();
        let mut w2 = Working::new();
        w2.restore_bytes(&snap).unwrap();
        assert_eq!(w2.len(), 1);
        assert_eq!(w2.entries[0].content, "v");
        assert_eq!(w2.token_budget, 64);
    }

    #[test]
    fn approx_tokens_grows_with_input() {
        assert_eq!(approx_tokens(""), 0);
        assert!(approx_tokens("hello") <= approx_tokens("hello world"));
    }
}
