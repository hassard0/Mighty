//! v0.26 Track C — `std.memory.Working` integration tests.

use mty_stdlib::memory::working::{approx_tokens, Working, DEFAULT_TOKEN_BUDGET};
use mty_stdlib::memory::MemoryHandle;

#[test]
fn working_render_markdown_shape() {
    let mut w = Working::new();
    w.push("plan", "outline the introduction");
    w.push("note", "user prefers concise output");
    let rendered = w.render();
    assert!(rendered.starts_with("## Working Memory\n"));
    assert!(rendered.contains("- **plan**: outline the introduction"));
    assert!(rendered.contains("- **note**: user prefers concise output"));
}

#[test]
fn working_empty_renders_empty_string() {
    let w = Working::new();
    assert_eq!(w.render(), "");
    assert!(w.is_empty());
}

#[test]
fn working_default_budget_matches_constant() {
    let w = Working::new();
    assert_eq!(w.token_budget, DEFAULT_TOKEN_BUDGET);
}

#[test]
fn working_budget_drops_oldest_when_exceeded() {
    // Very tight budget: each entry's approx tokens exceeds 4 so only
    // the most recent survives.
    let mut w = Working::with_budget(6);
    w.push("a", "first entry with significant text");
    w.push("b", "second entry with significant text");
    w.push("c", "third entry with significant text");
    assert_eq!(w.len(), 1);
    assert_eq!(w.entries[0].label, "c");
}

#[test]
fn working_clear_empties() {
    let mut w = Working::with_budget(100);
    w.push("a", "x");
    w.push("b", "y");
    w.clear();
    assert!(w.is_empty());
    // Budget preserved.
    assert_eq!(w.token_budget, 100);
}

#[test]
fn working_snapshot_round_trip() {
    let mut w = Working::with_budget(128);
    w.push("plan", "do the thing");
    w.push("ctx", "user is researcher");
    let snap = w.snapshot_bytes();

    let mut w2 = Working::new();
    w2.restore_bytes(&snap).unwrap();
    assert_eq!(w2.len(), 2);
    assert_eq!(w2.token_budget, 128);
    assert_eq!(w2.render(), w.render());
}

#[test]
fn working_zero_budget_clamps_to_one() {
    let w = Working::with_budget(0);
    assert_eq!(w.token_budget, 1);
}

#[test]
fn working_handle_kind_stable() {
    let w = Working::new();
    assert_eq!(w.kind(), "working");
}

#[test]
fn working_approx_tokens_monotone() {
    assert_eq!(approx_tokens(""), 0);
    assert!(approx_tokens("a") <= approx_tokens("ab"));
    assert!(approx_tokens("abcd") <= approx_tokens("abcdefgh"));
}

#[test]
fn working_current_tokens_tracks_pushes() {
    let mut w = Working::with_budget(1_000);
    assert_eq!(w.current_tokens(), 0);
    w.push("k", "v");
    assert!(w.current_tokens() > 0);
}
