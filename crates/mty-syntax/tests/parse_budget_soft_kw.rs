//! v0.29 Track E — `budget` is a SOFT (contextual) keyword.
//!
//! Pre-v0.29, `budget` was a hard reserved keyword: every real LLM-agent
//! demo had to rename its `budget` local to `spend_cap` because the
//! identifier was syntactically rejected. v0.29 demotes `budget` to a
//! contextual keyword, recognised by the parser only in expression
//! position followed by a `{ ident expr ... }` body. Everywhere else
//! `budget` is a plain identifier.
//!
//! These tests pin BOTH directions:
//!   - Identifier uses (let, fn param, struct field, method, builtin
//!     call) parse cleanly with ZERO errors.
//!   - The reserved `budget { cpu 150ms } run ...` block still parses as
//!     a BUDGET_BLOCK with the leading token re-tagged as BUDGET_KW.

use mty_syntax::parser::parse;
use mty_syntax::SyntaxNode;

fn parse_errors(src: &str) -> Vec<String> {
    let r = parse(src);
    r.errors.into_iter().map(|e| e.message).collect()
}

fn parse_dump(src: &str) -> String {
    let r = parse(src);
    let node = SyntaxNode::new_root(r.green);
    format!("{:#?}", node)
}

#[test]
fn budget_as_let_binding_parses_clean() {
    let src = "fn main() { let budget = 5.0; budget }";
    let errors = parse_errors(src);
    assert!(errors.is_empty(), "expected zero errors, got: {:?}", errors);
    let dump = parse_dump(src);
    // The identifier should appear as IDENT, not BUDGET_KW.
    assert!(
        dump.contains("IDENT") && dump.contains("\"budget\""),
        "expected IDENT \"budget\" in tree:\n{}",
        dump
    );
    // And no BUDGET_BLOCK node should be produced.
    assert!(
        !dump.contains("BUDGET_BLOCK"),
        "did not expect BUDGET_BLOCK in tree:\n{}",
        dump
    );
}

#[test]
fn budget_as_let_binding_mutable() {
    let errors = parse_errors("fn main() { let mut budget = 5.0; budget = budget + 1.0; }");
    assert!(errors.is_empty(), "got: {:?}", errors);
}

#[test]
fn budget_as_fn_parameter_parses_clean() {
    // Use `compute` (not `run`) for the fn name because `run` is itself
    // a reserved keyword. The point of this test is `budget` as a fn
    // PARAMETER.
    let src = "fn compute(budget: F64) -> F64 { budget }";
    let errors = parse_errors(src);
    assert!(errors.is_empty(), "got: {:?}", errors);
}

#[test]
fn budget_as_struct_field_parses_clean() {
    // `budget: F64` is a struct field — `budget` must be an identifier here.
    let src = "struct Plan { budget: F64, name: Str }";
    let errors = parse_errors(src);
    assert!(errors.is_empty(), "got: {:?}", errors);
}

#[test]
fn budget_as_struct_method_parses_clean() {
    // `fn budget(...)` declares a method named `budget`.
    let src = "impl Plan { fn budget(self) -> F64 { 0.0 } }";
    let errors = parse_errors(src);
    assert!(errors.is_empty(), "got: {:?}", errors);
}

#[test]
fn budget_as_struct_field_literal_value() {
    // `Plan { budget: 5.0 }` — `budget` is a field name in a struct literal.
    let src = "fn main() { let p = Plan { budget: 5.0 }; }";
    let errors = parse_errors(src);
    assert!(errors.is_empty(), "got: {:?}", errors);
}

#[test]
fn budget_as_method_call_target() {
    // `reviewer.budget(...)` — `budget` is a method name on a value.
    // (Avoiding `agent` as the receiver because that's a hard keyword.)
    let src = "fn main() { let x = reviewer.budget(10); x }";
    let errors = parse_errors(src);
    assert!(errors.is_empty(), "got: {:?}", errors);
    let dump = parse_dump(src);
    assert!(
        !dump.contains("BUDGET_BLOCK"),
        "did not expect BUDGET_BLOCK:\n{}",
        dump
    );
    // `reviewer.budget(10)` lands as a multi-segment PATH_EXPR call —
    // the `.budget` segment is a NAME_REF with IDENT "budget" inside a
    // PATH_SEGMENT (CALL_EXPR + PATH + DOT + PATH_SEGMENT + NAME_REF).
    // The point is: no errors, no BUDGET_BLOCK, and `budget` shows up
    // as an IDENT in a name-ref position.
    assert!(
        dump.contains("CALL_EXPR") && dump.contains("IDENT@") && dump.contains("\"budget\""),
        "expected call expr with IDENT \"budget\" name-ref in tree:\n{}",
        dump
    );
}

#[test]
fn budget_as_field_access() {
    // `plan.budget` — `budget` is a field name.
    let src = "fn main() { let b = plan.budget }";
    let errors = parse_errors(src);
    assert!(errors.is_empty(), "got: {:?}", errors);
}

#[test]
fn budget_in_call_position() {
    // `budget(5.0)` — `budget` is a function being called.
    let src = "fn main() { budget(5.0) }";
    let errors = parse_errors(src);
    assert!(errors.is_empty(), "got: {:?}", errors);
}

#[test]
fn budget_in_arithmetic() {
    // `budget + 1.0` — `budget` is an identifier in a binary expression.
    let src = "fn main() { let total = budget + 1.0; total }";
    let errors = parse_errors(src);
    assert!(errors.is_empty(), "got: {:?}", errors);
}

#[test]
fn budget_block_still_parses_as_reserved_form() {
    // The reserved budget-block syntax MUST keep working — this is the
    // very form that motivated keeping the soft keyword recognisable.
    let src = "fn main() { budget { cpu 150ms wall 2s } run job() }";
    let errors = parse_errors(src);
    assert!(errors.is_empty(), "got: {:?}", errors);
    let dump = parse_dump(src);
    assert!(
        dump.contains("BUDGET_BLOCK"),
        "expected BUDGET_BLOCK in tree:\n{}",
        dump
    );
    assert!(
        dump.contains("BUDGET_KW@"),
        "expected BUDGET_KW token tag in tree:\n{}",
        dump
    );
    assert!(
        dump.contains("BUDGET_ENTRY"),
        "expected BUDGET_ENTRY in tree:\n{}",
        dump
    );
}

#[test]
fn budget_block_empty_body_parses() {
    // `budget {} run ...` — the empty body shape. Parser must still
    // recognise it as a budget block (the runtime can reject it).
    let src = "fn main() { budget {} run job() }";
    let errors = parse_errors(src);
    assert!(errors.is_empty(), "got: {:?}", errors);
    let dump = parse_dump(src);
    assert!(
        dump.contains("BUDGET_BLOCK"),
        "expected BUDGET_BLOCK in tree:\n{}",
        dump
    );
}

#[test]
fn budget_then_assignment_is_identifier_not_block() {
    // `budget = ...` — definitely an identifier, no `{` follows.
    let src = "fn main() { let mut budget = 0.0; budget = 5.0; }";
    let errors = parse_errors(src);
    assert!(errors.is_empty(), "got: {:?}", errors);
    let dump = parse_dump(src);
    assert!(
        !dump.contains("BUDGET_BLOCK"),
        "did not expect BUDGET_BLOCK:\n{}",
        dump
    );
}

#[test]
fn budget_struct_literal_shape_not_block() {
    // `budget { x: 1 }` — looks superficially like a budget block but
    // the inside is `IDENT COLON ...`, which is the struct-literal
    // shape. We route through `path_expr_or_call` and parse as
    // STRUCT_EXPR (not BUDGET_BLOCK).
    //
    // (In practice no struct in Mighty is named lowercase `budget`, but
    // the disambiguation lookahead has to be precise enough that this
    // path can't accidentally fire the budget-block parser.)
    let src = "fn main() { let _ = budget { x: 1 } }";
    let dump = parse_dump(src);
    // Either STRUCT_EXPR or a clean error — but NOT BUDGET_BLOCK.
    assert!(
        !dump.contains("BUDGET_BLOCK"),
        "lookahead misfired — produced BUDGET_BLOCK:\n{}",
        dump
    );
}

#[test]
fn budget_in_complex_agent_state_parses_clean() {
    // The motivating example from demo 08: agent ctor / state with
    // `budget` as a field. Pre-v0.29 this hit the lexer's reserved-word
    // path and the demo had to rename to `spend_cap`.
    let src = r#"
agent Reviewer(budget) {
  state spent: F64 = 0.0

  on Tick() {
    let remaining = budget - spent;
    log(format!("{}", remaining))
  }
}
"#;
    let errors = parse_errors(src);
    assert!(errors.is_empty(), "got: {:?}", errors);
}

#[test]
fn budget_keyword_classifier_excludes_soft_kw() {
    // The lexer must never produce a BUDGET_KW token for the literal
    // string "budget" (it's a soft keyword now). Anything with text
    // "budget" should lex as IDENT.
    let src = "budget";
    let lexed = mty_syntax::lex(src);
    let kinds: Vec<_> = lexed.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![mty_syntax::SyntaxKind::IDENT, mty_syntax::SyntaxKind::EOF],
        "expected [IDENT, EOF] but got {:?}",
        kinds
    );
}
