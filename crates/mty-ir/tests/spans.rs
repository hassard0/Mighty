//! v0.22 — verify that the IR lowerer populates `Program::span_table`
//! for every fn it produces from real HIR, and that manually-constructed
//! `Program`s leave the table empty (back-compat).
//!
//! What this file asserts:
//!
//! 1. `parse_lower_preserves_spans` — small program with multiple
//!    statements; after lowering, the lowered fn has an entry in
//!    `Program::span_table` whose stmt_spans + terminator_span tables
//!    are populated (non-empty).
//! 2. `terminator_span_set` — a fn whose body ends with a return-expr
//!    has a terminator span recorded.
//! 3. `manually_constructed_program_default_span` — a hand-built
//!    `Function` (no HIR origin) leaves `Program::span_table` empty
//!    so the cranelift back-end falls back to the v0.21 synthetic
//!    spread, preserving back-compat for tests and the mono pass.
//! 4. `span_table_distinct_per_fn` — two fns lowered from HIR produce
//!    two separate `FnSpanTable` entries keyed by `IrFnId`.
//! 5. `span_lookup_helpers` — `FnSpanTable::stmt_span` /
//!    `terminator_span` accessors round-trip values set via the
//!    corresponding setters.

mod common;

use common::compile;
use mty_hir::SourceSpan;
use mty_ir::ir::{
    Block, BlockId, FnSpanTable, Function, IrFnId, IrTy, LocalDecl, LocalSource, Operand, Program,
    Stmt, Term,
};

#[test]
fn parse_lower_preserves_spans() {
    let src = "fn add_three(x: i32) -> i32 {\n  let a = x + 1\n  let b = a + 1\n  let c = b + 1\n  c\n}\n";
    let (_pkg, _typed, prog) = compile(src);
    let add = prog
        .fn_by_name("add_three")
        .expect("add_three fn should lower");
    let table = prog
        .span_table
        .get(&add.id)
        .expect("span_table should have entry for add_three");
    // Per-block stmt spans recorded.
    assert!(
        !table.stmt_spans.is_empty(),
        "stmt_spans should be populated for HIR-lowered fn"
    );
    // Terminator spans recorded.
    assert!(
        !table.terminator_spans.is_empty(),
        "terminator_spans should be populated for HIR-lowered fn"
    );
    // The entry block (idx 0) should have at least one stmt span and
    // a terminator span (the body is straight-line in this fn).
    let entry_stmt_spans = table
        .stmt_spans
        .get(&0u32)
        .expect("entry block must record stmt spans");
    assert!(
        !entry_stmt_spans.is_empty(),
        "entry block must have at least one recorded stmt span"
    );
    // Each recorded span should be inside the fn's overall span — we
    // can't yet guarantee per-stmt byte ranges (HIR doesn't expose
    // per-expr spans), but the fallback uses the fn's span so the
    // value must at least match `add.span`.
    for sp in entry_stmt_spans {
        assert_eq!(
            sp.start, add.span.start,
            "stmt span start should equal fn span start until HIR exposes per-expr spans"
        );
        assert_eq!(
            sp.end, add.span.end,
            "stmt span end should equal fn span end until HIR exposes per-expr spans"
        );
    }
    let term_span = table
        .terminator_spans
        .get(&0u32)
        .expect("entry block must record terminator span");
    assert_eq!(term_span.start, add.span.start);
    assert_eq!(term_span.end, add.span.end);
}

#[test]
fn terminator_span_set() {
    let src = "fn just_return() -> i32 {\n  return 42\n}\n";
    let (_pkg, _typed, prog) = compile(src);
    let f = prog
        .fn_by_name("just_return")
        .expect("just_return should lower");
    let table = prog
        .span_table
        .get(&f.id)
        .expect("span_table should have entry");
    // The return terminator's span must be set.
    assert!(
        !table.terminator_spans.is_empty(),
        "return-bodied fn must record at least one terminator span"
    );
}

#[test]
fn manually_constructed_program_default_span() {
    // Build a Function by hand (no HIR origin). The lowerer never
    // touches it, so `Program::span_table` must have no entry for
    // this fn — that's how the cranelift back-end falls back to the
    // v0.21 synthetic spread.
    let mut prog = Program::default();
    prog.fns.push(Function {
        id: IrFnId(0),
        name: "synth".into(),
        params: vec![],
        locals: vec![LocalDecl {
            name: "_ret".into(),
            ty: IrTy::Unit,
            mutable: true,
            source: LocalSource::Return,
        }],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![Stmt::Nop],
            terminator: Term::Return(Operand::Const(mty_ir::ir::Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Unit,
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });
    assert!(
        prog.span_table.is_empty(),
        "manually-built Program must leave span_table empty"
    );
    assert!(
        !prog.span_table.contains_key(&IrFnId(0)),
        "no entry should exist for manually-built fn"
    );
}

#[test]
fn span_table_distinct_per_fn() {
    let src = "fn one() -> i32 {\n  1\n}\n\nfn two() -> i32 {\n  2\n}\n";
    let (_pkg, _typed, prog) = compile(src);
    let one = prog.fn_by_name("one").expect("one");
    let two = prog.fn_by_name("two").expect("two");
    assert_ne!(one.id, two.id, "fn ids must differ");
    let one_tbl = prog
        .span_table
        .get(&one.id)
        .expect("one must have a span table");
    let two_tbl = prog
        .span_table
        .get(&two.id)
        .expect("two must have a span table");
    // Each fn's recorded terminator span must reflect the corresponding
    // fn's span — they must NOT collide.
    let one_term = one_tbl
        .terminator_spans
        .get(&0u32)
        .expect("one has terminator span");
    let two_term = two_tbl
        .terminator_spans
        .get(&0u32)
        .expect("two has terminator span");
    assert_eq!(one_term.start, one.span.start);
    assert_eq!(two_term.start, two.span.start);
    assert_ne!(
        one.span.start, two.span.start,
        "two distinct fns should have distinct source positions"
    );
}

#[test]
fn span_lookup_helpers() {
    // Round-trip values through the FnSpanTable API so future
    // refactors of the side-table can't silently lose data.
    let mut t = FnSpanTable::new();
    let s = SourceSpan { start: 42, end: 99 };
    t.set_stmt_span(7, 3, s.clone());
    t.set_terminator_span(7, s.clone());

    let got = t.stmt_span(7, 3).expect("stmt span readback");
    assert_eq!(got.start, 42);
    assert_eq!(got.end, 99);
    let got_term = t.terminator_span(7).expect("term span readback");
    assert_eq!(got_term.start, 42);
    assert_eq!(got_term.end, 99);

    // Out-of-range / missing lookups return None.
    assert!(
        t.stmt_span(7, 99).is_none(),
        "missing stmt_idx returns None"
    );
    assert!(t.stmt_span(99, 0).is_none(), "missing block returns None");
    assert!(
        t.terminator_span(99).is_none(),
        "missing terminator block returns None"
    );
}
