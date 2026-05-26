//! Parser-only acceptance for RFC-008 effect-row surface syntax (v0.15).
//!
//! These tests pin the SHAPE of the CST that `parser::types::effect_clause`
//! emits when it sees the new forms:
//!
//!   * `!E`               — bare row var
//!   * `!{}`              — empty closed row (also legacy)
//!   * `!{fs}`            — single concrete effect
//!   * `!{fs, net}`       — multiple concrete effects
//!   * `!{fs | E}`        — concrete + row tail
//!   * `!{fs, net | E}`   — multiple concrete + row tail
//!   * `!{| E}`           — only a row tail (semantically equivalent to `!E`)
//!
//! And legacy back-compat:
//!
//!   * `effect fs, net`           — keyword form
//!   * `effect fs, net | E`       — keyword form + row tail (NEW in v0.15)
//!   * `Page!{NetErr, ParseErr}`  — anonymous error union, still error sugar
//!   * `Page!FetchErr`            — bare error sugar, still error sugar
//!
//! HIR lowering / typeck wiring for the new nodes lands in v0.16
//! (per the RFC-008 v0.14 follow-up entry).

use mty_syntax::{parse, SyntaxKind, SyntaxNode};

fn dump(src: &str) -> String {
    let r = parse(src);
    let node = SyntaxNode::new_root(r.green);
    format!("{:#?}\nerrors: {:?}", node, r.errors)
}

fn errors_of(src: &str) -> Vec<String> {
    parse(src).errors.into_iter().map(|e| e.message).collect()
}

/// True iff any descendant node of `src`'s parse has the given kind.
fn contains_kind(src: &str, kind: SyntaxKind) -> bool {
    let r = parse(src);
    let node = SyntaxNode::new_root(r.green);
    node.descendants().any(|n| n.kind() == kind)
}

// ---------------- Acceptance: new `!{...}` forms ----------------

#[test]
fn parse_bare_row_var() {
    // `!E` after a unit return type. `()` is TYPE_TUPLE so the `!`
    // can't be greedily consumed by the type-side error-sugar path.
    let src = "fn f() -> () !E { }";
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected no parse errors, got {errs:?}");
    assert!(
        contains_kind(src, SyntaxKind::EFFECT_ROW_VAR),
        "expected EFFECT_ROW_VAR node\n{}",
        dump(src)
    );
}

#[test]
fn parse_concrete_plus_row() {
    let src = "fn f() -> () !{fs | E} { }";
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected no parse errors, got {errs:?}");
    assert!(
        contains_kind(src, SyntaxKind::EFFECT_SET),
        "expected EFFECT_SET node\n{}",
        dump(src)
    );
    assert!(
        contains_kind(src, SyntaxKind::EFFECT_NAME),
        "expected EFFECT_NAME node\n{}",
        dump(src)
    );
    assert!(
        contains_kind(src, SyntaxKind::EFFECT_ROW_TAIL),
        "expected EFFECT_ROW_TAIL node\n{}",
        dump(src)
    );
    assert!(
        contains_kind(src, SyntaxKind::EFFECT_ROW_VAR),
        "expected EFFECT_ROW_VAR node\n{}",
        dump(src)
    );
}

#[test]
fn parse_multiple_effects_plus_row() {
    let src = "fn f() -> () !{fs, net, time | E} { }";
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected no parse errors, got {errs:?}");
    // Three EFFECT_NAME nodes, one EFFECT_ROW_TAIL.
    let r = parse(src);
    let node = SyntaxNode::new_root(r.green);
    let names: Vec<_> = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::EFFECT_NAME)
        .collect();
    assert_eq!(names.len(), 3, "expected 3 EFFECT_NAME, got {names:?}");
}

#[test]
fn parse_empty_braced_row() {
    let src = "fn f() -> () !{} { }";
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected no parse errors, got {errs:?}");
    assert!(
        contains_kind(src, SyntaxKind::EFFECT_SET),
        "expected EFFECT_SET node\n{}",
        dump(src)
    );
}

#[test]
fn parse_concrete_only_braced() {
    let src = "fn f() -> () !{fs, net} { }";
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected no parse errors, got {errs:?}");
    assert!(contains_kind(src, SyntaxKind::EFFECT_SET));
    assert!(!contains_kind(src, SyntaxKind::EFFECT_ROW_TAIL));
}

#[test]
fn parse_row_tail_only_braced() {
    // `!{ | E }` — row var only, in braced form. Semantically equivalent
    // to `!E` but accepted because RFC-008 grammar allows it.
    let src = "fn f() -> () !{ | E } { }";
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected no parse errors, got {errs:?}");
    assert!(contains_kind(src, SyntaxKind::EFFECT_ROW_TAIL));
}

#[test]
fn parse_row_on_path_return_type() {
    // `List[B] !E` — return type is a path-type, not unit. The
    // disambiguation rule keeps `!E` (no braces) as error sugar even
    // here, so callers who want a row var on a path return type must
    // use the braced form OR the legacy `effect E` keyword form.
    // This test pins THAT behaviour (back-compat).
    let src = "fn f() -> List[B] !{fs | E} { }";
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected no parse errors, got {errs:?}");
    assert!(contains_kind(src, SyntaxKind::EFFECT_SET));
    assert!(contains_kind(src, SyntaxKind::EFFECT_ROW_TAIL));
    // Crucially: NOT wrapped in TYPE_RESULT_SUGAR.
    let r = parse(src);
    let node = SyntaxNode::new_root(r.green);
    let has_sugar_outside_effect_set = node
        .descendants()
        .any(|n| n.kind() == SyntaxKind::TYPE_RESULT_SUGAR);
    assert!(
        !has_sugar_outside_effect_set,
        "`!{{fs | E}}` should not be treated as TYPE_RESULT_SUGAR\n{}",
        dump(src)
    );
}

#[test]
fn parse_row_on_generic_decl() {
    // The full RFC-008 motivating example: a row-polymorphic HOF.
    let src = "fn map[A, B, E](xs: List[A], f: fn(A) -> B) -> List[B] !{fs | E} { [] }";
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected no parse errors, got {errs:?}");
    assert!(contains_kind(src, SyntaxKind::EFFECT_ROW_VAR));
    // `E` shows up in BOTH the generic param list AND the row tail.
    // The parser doesn't link them (that's typeck) but both should
    // be present in the CST.
}

// ---------------- Back-compat: legacy forms ----------------

#[test]
fn parse_no_row_var_legacy_keyword() {
    // Existing form: `effect fs, net`. Must keep parsing AND keep its
    // existing CST shape (NAME directly under EFFECT_CLAUSE — see the
    // back-compat note in `effect_clause_keyword`).
    let src = "fn f() -> Page effect net, time { }";
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected no parse errors, got {errs:?}");
    assert!(contains_kind(src, SyntaxKind::EFFECT_CLAUSE));
    // CRITICAL: no EFFECT_NAME wrapper on the legacy form (HIR
    // lowerer at `mty-hir::lower::items::lower_fn` calls
    // `Name::cast` directly on EFFECT_CLAUSE children — wrapping
    // would silently lose all effect names).
    assert!(
        !contains_kind(src, SyntaxKind::EFFECT_NAME),
        "legacy `effect a, b` must keep bare NAME children for HIR back-compat\n{}",
        dump(src)
    );
}

#[test]
fn parse_keyword_form_with_row_tail() {
    // v0.15 NEW: the keyword form `effect a, b | E` is also accepted.
    // This is a convenience for users migrating from `effect ...` who
    // want a row tail without rewriting to `!{...}`.
    let src = "fn f() -> Page effect net, time | E { }";
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected no parse errors, got {errs:?}");
    assert!(contains_kind(src, SyntaxKind::EFFECT_ROW_TAIL));
    assert!(contains_kind(src, SyntaxKind::EFFECT_ROW_VAR));
}

#[test]
fn parse_legacy_error_sugar_still_works() {
    // `Page!{NetErr, ParseErr}` — the disambiguation rule (first ident
    // uppercase + no `|`) keeps this as legacy error sugar so
    // example 04 doesn't regress.
    let src = "fn f() -> Page!{NetErr, ParseErr} { Page() }";
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected no parse errors, got {errs:?}");
    assert!(
        contains_kind(src, SyntaxKind::TYPE_RESULT_SUGAR),
        "`!{{NetErr, ParseErr}}` must stay TYPE_RESULT_SUGAR\n{}",
        dump(src)
    );
    assert!(
        !contains_kind(src, SyntaxKind::EFFECT_SET),
        "`!{{NetErr, ParseErr}}` must NOT be an EFFECT_SET\n{}",
        dump(src)
    );
}

#[test]
fn parse_legacy_bare_error_sugar_still_works() {
    let src = "fn f() -> Page!FetchErr { Page() }";
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected no parse errors, got {errs:?}");
    assert!(
        contains_kind(src, SyntaxKind::TYPE_RESULT_SUGAR),
        "`!FetchErr` must stay TYPE_RESULT_SUGAR\n{}",
        dump(src)
    );
}

// ---------------- Rejection / error paths ----------------

#[test]
fn reject_pipe_with_no_row_var() {
    // `!{fs | }` — the `|` introduces a row tail, but no row var
    // identifier follows. Parser surfaces a diagnostic.
    let src = "fn f() -> () !{fs | } { }";
    let errs = errors_of(src);
    assert!(
        errs.iter().any(|m| m.contains("row variable")),
        "expected a row-var diagnostic, got {errs:?}"
    );
}

#[test]
fn reject_bang_with_nonsense_after() {
    // `!` followed by something that's neither `{` nor an identifier.
    // Parser must surface a diagnostic rather than silently producing
    // a malformed EFFECT_CLAUSE.
    //
    // We use `!;` here because `! { }` is intentionally ambiguous —
    // by RFC-008's `!{}` form, `{ }` after `!` IS a valid (empty)
    // effect set, so the parser greedily consumes the braces; the
    // ensuing body-missing condition is reported by fn_decl_pub, not
    // effect_clause.
    let src = "fn f() -> () !; { }";
    let r = parse(src);
    assert!(
        !r.errors.is_empty(),
        "expected a parse error for `!;` (no effect set or row var)\n{}",
        SyntaxNode::new_root(r.green)
    );
}

/// Smoke-test: the v0.15 effect-row example file in `examples/`
/// parses without errors. The file is gated for full typeck (see the
/// `// @typeck-pending` markers) but MUST parse clean today.
#[test]
fn example_22_effect_row_parses_clean() {
    // CARGO_MANIFEST_DIR is `.../crates/mty-syntax`; walk up twice.
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("examples")
        .join("22_effect_row.mty");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = parse(&src);
    assert!(
        r.errors.is_empty(),
        "examples/22_effect_row.mty should parse clean; got {:?}\n{}",
        r.errors,
        SyntaxNode::new_root(r.green)
    );
}

#[test]
fn parse_empty_braced_row_then_body() {
    // `!{} { ... }` — the explicit empty-braces form followed by the
    // body. Disambiguates the `! { }` ambiguity from the rejection
    // test above.
    let src = "fn f() -> () !{} { }";
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected no parse errors, got {errs:?}");
    assert!(contains_kind(src, SyntaxKind::EFFECT_SET));
    // And the body block survived.
    assert!(contains_kind(src, SyntaxKind::BLOCK));
}
