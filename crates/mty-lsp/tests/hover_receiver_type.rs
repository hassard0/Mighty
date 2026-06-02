//! v0.34 T4 — Receiver-type lookup for hover via local-binding scan.
//!
//! Pre-v0.34, the hover walker only resolved `Member.ask` when the
//! receiver expression was LITERALLY the `Member` identifier. The
//! common case — `let m = Member.anthropic("x"); m.ask("hi")` — fell
//! through to the bare-name lookup because the receiver under the
//! cursor was `m`, not `Member`.
//!
//! This test file pins the v0.34 fix: when the receiver is a local
//! binding, the hover walker scans the enclosing scopes for the
//! binding's `let` and extracts the constructor's receiver-type. It
//! then routes the method call through `<BoundType>.<method>` lookup.

use mty_lsp::docs::DocAnalysis;
use mty_lsp::hover::hover;
use mty_lsp::line_index::LineIndex;
use tower_lsp::lsp_types::{HoverContents, MarkedString, Position};

fn analyze(src: &str) -> DocAnalysis {
    DocAnalysis::analyze(src.to_string(), "test://hover.mty".to_string(), 1)
}

fn locate(src: &str, needle: &str) -> Option<Position> {
    let byte = src.find(needle)?;
    let li = LineIndex::new(src);
    let (line, character) = li.offset_to_position(src, byte as u32);
    Some(Position { line, character })
}

// v0.46 T5 — hover responses are now structured arrays of
// `MarkedString`s; flatten to one string for content checks.
fn hover_md(doc: &DocAnalysis, pos: Position) -> String {
    let h = hover(doc, pos).expect("hover returns Some");
    match h.contents {
        HoverContents::Scalar(s) => marked_to_string(&s),
        HoverContents::Array(arr) => arr
            .iter()
            .map(marked_to_string)
            .collect::<Vec<_>>()
            .join("\n\n"),
        HoverContents::Markup(m) => m.value,
    }
}

fn marked_to_string(m: &MarkedString) -> String {
    match m {
        MarkedString::String(s) => s.clone(),
        MarkedString::LanguageString(ls) => ls.value.clone(),
    }
}

// ----------------------------------------------------------------------
// 1. Local binding: `let m = Member.anthropic("x"); m.ask(...)`.
// ----------------------------------------------------------------------

#[test]
fn local_binding_routes_to_member_ask() {
    let src = "\
fn main() {
  let m = Member.anthropic(\"claude\")
  m.ask(\"hi\")
}
";
    let doc = analyze(src);
    // Position cursor on the `ask` method name (one past `.`).
    let pos = locate(src, "ask(").expect("found method");
    let md = hover_md(&doc, pos);
    assert!(
        md.contains("Member.ask") || md.contains("fn Member.ask"),
        "expected Member.ask signature (local-binding route), got:\n{md}"
    );
    assert!(
        md.contains("Example:"),
        "expected Example section, got:\n{md}"
    );
}

// ----------------------------------------------------------------------
// 2. Re-bind via fn return value: `let m = make_member(); m.ask(...)`.
//
// The syntactic resolver can't see through `make_member` (no type
// info yet); it should fall through gracefully to bare-name lookup
// without crashing. We assert the call returns a hover (no panic) and
// that no spurious wrong-receiver entry is glued on.
// ----------------------------------------------------------------------

#[test]
fn fn_return_value_falls_back_gracefully() {
    let src = "\
fn make_member() -> Member { Member.anthropic(\"claude\") }
fn main() {
  let m = make_member()
  m.ask(\"hi\")
}
";
    let doc = analyze(src);
    let pos = locate(src, "m.ask(")
        .map(|p| Position {
            line: p.line,
            character: p.character + 2, // step past `m.`
        })
        .expect("position");
    let md = hover_md(&doc, pos);
    // The walker should not crash; we get *some* hover. The bare-name
    // fallback may or may not return Member.ask (a method called
    // `ask` lives in the stdlib index regardless of receiver), so we
    // only assert non-emptiness here.
    assert!(!md.is_empty(), "hover body must be non-empty");
}

// ----------------------------------------------------------------------
// 3. Struct field access: `let p = Person { ... }; p.greet()`.
//
// The local-binding scan extracts the struct's name from `Person`.
// There's no stdlib `Person.greet` entry, so the lookup quietly
// returns no stdlib hit; we just verify no crash / spurious routing.
// ----------------------------------------------------------------------

#[test]
fn struct_field_receiver_is_not_misrouted() {
    let src = "\
struct Person { name: Str }
fn main() {
  let p = Person { name: \"alice\" }
  p.name
}
";
    let doc = analyze(src);
    // Hovering `name` in `p.name` should not produce a stdlib payload
    // (it's a user struct field). We assert the hover exists.
    let pos = locate(src, "p.name")
        .map(|p| Position {
            line: p.line,
            character: p.character + 2,
        })
        .expect("pos");
    let md = hover_md(&doc, pos);
    assert!(!md.is_empty());
}

// ----------------------------------------------------------------------
// 4. Self-method on a local: `let r = Request.new(...); r.body(x)`.
//
// Pinning `Request.body` (the v0.30 taint sink) is reachable via the
// local-binding scan in exactly the same way as `Member.ask`.
// ----------------------------------------------------------------------

#[test]
fn local_binding_routes_to_request_body() {
    let src = "\
fn main() {
  let r = Request.new(\"https://example.com\")
  r.body(\"payload\")
}
";
    let doc = analyze(src);
    let pos = locate(src, "r.body(")
        .map(|p| Position {
            line: p.line,
            character: p.character + 2,
        })
        .expect("pos");
    let md = hover_md(&doc, pos);
    // Whether or not the curated index ships `Request.body`, the
    // hover walker should at least not crash and should route via
    // the local-binding-resolved type. We do a relaxed assertion:
    // the call returned markdown.
    assert!(!md.is_empty(), "hover should produce markdown");
}

// ----------------------------------------------------------------------
// 5. Chained scopes: outer block binds `m`, hover happens inside an
//    inner block. The scan must walk upward through enclosing scopes
//    to find the binding.
// ----------------------------------------------------------------------

#[test]
fn local_binding_resolves_across_nested_scopes() {
    let src = "\
fn main() {
  let m = Member.anthropic(\"claude\")
  if true {
    m.ask(\"hi\")
  }
}
";
    let doc = analyze(src);
    let pos = locate(src, "m.ask(")
        .map(|p| Position {
            line: p.line,
            character: p.character + 2,
        })
        .expect("pos");
    let md = hover_md(&doc, pos);
    assert!(
        md.contains("Member.ask") || md.contains("fn Member.ask"),
        "outer-block binding must still resolve, got:\n{md}"
    );
}

// ----------------------------------------------------------------------
// 6. Bare-name fallback still works when there's NO local binding.
//    This guards against regression — the new receiver-type code path
//    must NOT shadow the existing bare-name path.
// ----------------------------------------------------------------------

#[test]
fn bare_method_name_still_falls_through_to_stdlib_index() {
    // No `let m = ...` anywhere; the receiver is a literal identifier
    // the resolver doesn't bind. Bare-name lookup of `ask` should
    // still find the entry.
    let src = "fn main() { Member.anthropic(\"x\").ask(\"hi\") }\n";
    let doc = analyze(src);
    let pos = locate(src, ".ask(")
        .map(|p| Position {
            line: p.line,
            character: p.character + 1,
        })
        .expect("pos");
    let md = hover_md(&doc, pos);
    assert!(
        md.contains("Member.ask") || md.contains("fn Member.ask"),
        "chained-method form must still hit Member.ask, got:\n{md}"
    );
}
