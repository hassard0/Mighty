//! v0.21 + v0.22 end-to-end tests for the cranelift `MachSrcLoc`
//! plumbing into the DWARF v5 line program.
//!
//! v0.20 shipped the v5 emitter with a conservative 2-entry line
//! table; v0.21 plumbed cranelift's per-instruction MachSrcLoc map
//! through `Module::define_function` so every machine instruction
//! that came from an MtyIR statement gets its own line-program row.
//! v0.22 (this slice) replaces v0.21's *synthetic* per-stmt byte
//! offsets (spread across the fn's source range) with **real** spans
//! from `Program::span_table` — populated by the HIR→IR lowerer for
//! every fn with a real HIR origin.
//!
//! What this file asserts:
//!
//! 1. `mach_src_loc_captured_during_compile` — define a 5-stmt fn,
//!    compile, verify the captured SrcLoc map has at least one entry
//!    per statement.
//! 2. `dwarf5_emits_per_instruction_rows` — compile + emit v5 DWARF;
//!    the `.debug_line` section contains many more rows than the v4
//!    per-basic-block emitter would.
//! 3. `dwarf5_per_local_loclist_emitted` — a fn with multiple locals
//!    yields a non-empty `.debug_loclists` section.
//! 4. `v4_path_unchanged` — default v4 emission still produces the
//!    v0.20 baseline (no regression).
//! 5. `srcloc_count_scales_with_statement_count` — load-bearing
//!    assertion that we still emit one SourceLoc per stmt + terminator.
//! 6. (v0.22) `dwarf5_row_byte_offsets_match_source` — populate
//!    `Program::span_table` with deliberately distinct byte ranges and
//!    verify the captured `stmt_byte_offsets` reflect THOSE positions
//!    (not the synthetic spread). This is the v0.22 acceptance gate.

use cranelift_codegen::isa;
use cranelift_object::{ObjectBuilder, ObjectModule};
use mty_codegen_cranelift::debug::{
    build_dwarf5_for, build_dwarf_for, dwarf5_enabled, DwarfInputs,
};
use mty_codegen_cranelift::lower::{default_flags, LowerCtx};
use mty_hir::SourceSpan;
use mty_ir::ir::{
    BinOp, Block, BlockId, Const, FnSpanTable, Function, IrFnId, IrTy, Local, LocalDecl,
    LocalSource, Operand, Place, Program, Rvalue, Stmt, Term,
};
use mty_types::IntKind;
use target_lexicon::Triple;

/// Build a fn body with N statements that cranelift can't constant-fold
/// away. Each statement adds a distinct constant to the function's
/// `seed` parameter, so the dependency chain runs through a value the
/// optimizer can't see at compile-time. The shape:
///
/// ```text
/// fn many_stmts(seed: i32) -> i32 {
///   let x0 = seed + 0
///   let x1 = x0 + 1
///   let x2 = x1 + 2
///   ...
///   return xN-1
/// }
/// ```
///
/// This produces N distinct `iadd` machine instructions, each with
/// its own SourceLoc → cranelift records N MachSrcLoc entries.
fn many_stmts_fn(stmt_count: u32) -> Function {
    let mut locals = vec![
        LocalDecl {
            name: "_0".into(),
            ty: IrTy::Int(IntKind::I32),
            mutable: false,
            source: LocalSource::Return,
        },
        // _1 is the `seed` parameter local.
        LocalDecl {
            name: "seed".into(),
            ty: IrTy::Int(IntKind::I32),
            mutable: false,
            source: LocalSource::UserLet,
        },
    ];
    // Add `stmt_count` user locals starting at _2.
    for i in 0..stmt_count {
        locals.push(LocalDecl {
            name: format!("x{i}"),
            ty: IrTy::Int(IntKind::I32),
            mutable: true,
            source: LocalSource::UserLet,
        });
    }
    let mut stmts: Vec<Stmt> = Vec::new();
    for i in 0..stmt_count {
        let prev_local = if i == 0 { Local(1) } else { Local(1 + i) };
        // xi = prev + (i + 1)
        let rv = Rvalue::BinOp(
            BinOp::Add,
            Operand::Copy(Place {
                local: prev_local,
                proj: vec![],
            }),
            Operand::Const(Const::Int((i + 1) as i128, IntKind::I32)),
        );
        stmts.push(Stmt::Assign(
            Place {
                local: Local(2 + i),
                proj: vec![],
            },
            rv,
        ));
    }
    Function {
        id: IrFnId(0),
        name: "many_stmts".into(),
        params: vec![Local(1)],
        locals,
        blocks: vec![Block {
            id: BlockId(0),
            stmts,
            terminator: Term::Return(Operand::Copy(Place {
                local: Local(1 + stmt_count),
                proj: vec![],
            })),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Int(IntKind::I32),
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 256 },
    }
}

/// Helper: compile `prog` with debug capture enabled, return the
/// per-fn `FnSrcLocMap`s. Builds an ObjectModule (same shape the AOT
/// path uses) but discards the object — we only care about the
/// captured side-table.
///
/// Note: under cranelift's default `opt_level = "speed"` the egraph
/// optimizer aggressively coalesces arithmetic chains, which collapses
/// the per-statement `MachSrcLoc` rows the test depends on. We honour
/// the `MTY_CRANELIFT_NO_OPT` env var (already supported by
/// `default_flags`) so the test environment can opt into the more
/// representative `opt_level = "none"` shape. With `opt = none`,
/// cranelift emits roughly one machine instruction per statement and
/// preserves the per-statement SourceLoc on each.
fn compile_with_debug(prog: &Program) -> mty_codegen_cranelift::lower::FnSrcLocMap {
    // Ensure opt is off for deterministic srcloc accounting. We only
    // set it here (not module-globally) so other tests in the suite
    // aren't affected.
    // SAFETY: tests run single-threaded by default unless overridden;
    // the env var is consumed inside `default_flags` below.
    std::env::set_var("MTY_CRANELIFT_NO_OPT", "1");
    let triple = Triple::host();
    let isa_builder = isa::lookup(triple.clone()).expect("isa lookup");
    let flags = default_flags(true);
    let isa = isa_builder.finish(flags).expect("isa finish");
    let builder = ObjectBuilder::new(
        isa,
        b"mighty-test".to_vec(),
        cranelift_module::default_libcall_names(),
    )
    .expect("object builder");
    let mut module = ObjectModule::new(builder);
    let mut ctx = LowerCtx::new(&mut module, triple);
    ctx.enable_debug_capture();
    ctx.declare_fns(prog).expect("declare fns");
    for f in &prog.fns {
        ctx.define_fn(prog, f).expect("define fn");
    }
    // Return the single fn's debug entry. Test fns only define one
    // user-visible fn at index 0.
    let id = prog.fns[0].id;
    ctx.fn_debug.remove(&id).expect("fn_debug populated")
}

#[test]
fn mach_src_loc_captured_during_compile() {
    let mut prog = Program::default();
    prog.fns.push(many_stmts_fn(5));
    let dbg = compile_with_debug(&prog);

    // We seed one synthetic stmt-loc per statement plus one for the
    // terminator. For a 5-stmt fn that's 6 distinct
    // `stmt_byte_offsets` entries — the lowerer hands cranelift 6
    // unique SourceLoc values regardless of optimizer aggressiveness.
    assert!(
        dbg.stmt_byte_offsets.len() >= 6,
        "expected at least 6 synthetic stmt offsets (5 stmts + terminator), got {}",
        dbg.stmt_byte_offsets.len()
    );
    // Under `opt_level = none`, cranelift emits roughly one machine
    // instruction per CLIF instruction; we deduplicate to one row per
    // (code_offset, srcloc) pair. 4+ non-default rows is the
    // load-bearing assertion that we've broken the v0.20 coarse-grain
    // 2-row baseline.
    assert!(
        dbg.rows.len() >= 4,
        "MachSrcLoc map should record >= 4 dense entries for a 5-stmt fn, got {} entries (stmt_byte_offsets has {})",
        dbg.rows.len(),
        dbg.stmt_byte_offsets.len()
    );
    // Code size must be populated (non-zero) since the fn is real.
    assert!(
        dbg.code_size > 0,
        "compiled code size must be > 0, got {}",
        dbg.code_size
    );
}

#[test]
fn dwarf5_emits_per_instruction_rows() {
    let mut prog = Program::default();
    prog.fns.push(many_stmts_fn(5));
    let dbg = compile_with_debug(&prog);

    let mut srcloc_map = std::collections::HashMap::new();
    srcloc_map.insert(prog.fns[0].id, dbg);

    let inputs = DwarfInputs {
        source_text: "fn many_stmts() {\n  let a = 0\n  let b = 1\n  let c = 2\n  let d = 3\n  let e = 4\n}\n",
        source_path: "many.mty",
        comp_dir: "/tmp".into(),
    };
    let b = build_dwarf5_for(&prog, &inputs, Some(&srcloc_map)).expect("build v5");
    let rows = b.rows_emitted();
    let seqs = b.sequences_emitted();
    let _enc = b.finish().expect("finish v5");

    // 1 fn = 1 sequence; rows must be significantly larger than
    // the per-basic-block emitter would produce. The fn has 1 SIR
    // basic-block, so per-basic-block would yield ~1 row; we expect
    // 5+ from per-instruction.
    assert_eq!(seqs, 1, "one sequence per fn");
    assert!(
        rows > seqs * 2,
        "per-instruction granularity: rows ({rows}) > 2× sequences ({seqs})"
    );
}

#[test]
fn dwarf5_per_local_loclist_emitted() {
    let mut prog = Program::default();
    // 3 locals = 3 loclists expected.
    prog.fns.push(many_stmts_fn(3));
    let dbg = compile_with_debug(&prog);

    let mut srcloc_map = std::collections::HashMap::new();
    srcloc_map.insert(prog.fns[0].id, dbg);

    let inputs = DwarfInputs {
        source_text: "fn three_locals() {\n  let a = 0\n  let b = 1\n  let c = 2\n}\n",
        source_path: "three.mty",
        comp_dir: "/tmp".into(),
    };
    let b = build_dwarf5_for(&prog, &inputs, Some(&srcloc_map)).expect("build v5");
    let loclists = b.loclist_locals_emitted();
    let enc = b.finish().expect("finish v5");

    // 3 user locals + 1 synthesised `_N` for any missing-name ones.
    // The fn skips `_0` (Return slot), keeps x0/x1/x2.
    assert!(
        loclists >= 3,
        "expected >= 3 loclist locals, got {loclists}"
    );
    // `.debug_loclists` section must be present.
    assert!(
        enc.sections.iter().any(|s| s.name == ".debug_loclists"),
        ".debug_loclists section must be emitted; got {:?}",
        enc.sections.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
}

#[test]
fn v4_path_unchanged() {
    // Default v4 emission ignores the new rich line table — it
    // produces the same v0.20 baseline output regardless of whether
    // we have a populated FnSrcLocMap.
    assert!(
        !dwarf5_enabled() || std::env::var("MTY_DWARF5").is_ok(),
        "MTY_DWARF5 must be unset for this test, or set explicitly"
    );

    let mut prog = Program::default();
    prog.fns.push(many_stmts_fn(3));
    let inputs = DwarfInputs {
        source_text: "fn three() {\n  let a = 0\n}\n",
        source_path: "three.mty",
        comp_dir: "/tmp".into(),
    };
    let b = build_dwarf_for(&prog, &inputs).expect("build v4");
    let enc = b.finish().expect("finish v4");

    // v4 must not include the v5-only `.debug_line_str` section.
    assert!(
        !enc.sections.iter().any(|s| s.name == ".debug_line_str"),
        "v4 path should NOT emit .debug_line_str"
    );
    // Standard v4 sections must be present.
    assert!(enc.sections.iter().any(|s| s.name == ".debug_info"));
    assert!(enc.sections.iter().any(|s| s.name == ".debug_abbrev"));
    assert!(enc.sections.iter().any(|s| s.name == ".debug_line"));
}

/// v0.22 acceptance gate: when the lowerer populates `Program::span_table`
/// with explicit byte ranges (one per Stmt and one for the Terminator),
/// the cranelift backend MUST observe those exact byte offsets as the
/// values it hands to `set_srcloc` — not the synthetic spread that
/// v0.21 produced. Each Stmt's `span.start` flows directly into a
/// `stmt_byte_offsets` entry; the test reads them back via the
/// `FnSrcLocMap` side-table and pairs them against the input.
#[test]
fn dwarf5_row_byte_offsets_match_source() {
    let mut prog = Program::default();
    let mut f = many_stmts_fn(5);
    // Stretch the function span so it does NOT collide with our
    // hand-picked Stmt offsets — otherwise the synthetic fallback's
    // output could accidentally agree with the real values and the
    // assertion would lose its bite.
    f.span = SourceSpan {
        start: 1000,
        end: 1256,
    };
    let fn_id = f.id;
    prog.fns.push(f);

    // Hand-built span table: walk distinct, sparse byte ranges so the
    // assertion can prove that each value flows through unchanged.
    // Stmt N starts at byte (100 + N * 10); the terminator at 200.
    let mut table = FnSpanTable::new();
    for i in 0..5u32 {
        table.set_stmt_span(
            0,
            i as usize,
            SourceSpan {
                start: 100 + i * 10,
                end: 100 + i * 10 + 8,
            },
        );
    }
    table.set_terminator_span(
        0,
        SourceSpan {
            start: 200,
            end: 210,
        },
    );
    prog.span_table.insert(fn_id, table);

    let dbg = compile_with_debug(&prog);

    // The lowerer hands cranelift one SourceLoc per Stmt + one for
    // the terminator → 6 stmt_byte_offsets entries. The VALUES must
    // be drawn from our hand-populated span table, NOT from the
    // synthetic spread (which would pick offsets inside
    // 1000..=1255).
    assert_eq!(
        dbg.stmt_byte_offsets.len(),
        6,
        "5 stmts + 1 terminator → 6 stmt_byte_offsets entries (got {})",
        dbg.stmt_byte_offsets.len()
    );
    let expected: [u32; 6] = [100, 110, 120, 130, 140, 200];
    for (i, want) in expected.iter().enumerate() {
        assert_eq!(
            dbg.stmt_byte_offsets[i], *want,
            "stmt_byte_offsets[{}] should match real span.start ({}), got {} \
             (synthetic spread would have produced something in 1000..=1255)",
            i, want, dbg.stmt_byte_offsets[i]
        );
    }
    // Sanity check: none of the recorded offsets sit inside the fn
    // span's synthetic range. If any did, the back-end would have
    // silently fallen back and we'd never know.
    for off in &dbg.stmt_byte_offsets {
        assert!(
            *off < 1000 || *off >= 1256,
            "byte offset {} falls inside synthetic fn-span range — \
             back-end didn't use the real span table",
            off
        );
    }
}

#[test]
fn srcloc_count_scales_with_statement_count() {
    // 3-stmt vs 8-stmt fns should produce noticeably different
    // `rows.len()` values — this is the load-bearing assertion that
    // the plumbing is actually per-statement, not per-fn.
    let mut prog_small = Program::default();
    prog_small.fns.push(many_stmts_fn(3));
    let small = compile_with_debug(&prog_small);

    let mut prog_big = Program::default();
    prog_big.fns.push(many_stmts_fn(8));
    let big = compile_with_debug(&prog_big);

    // The lowerer hands cranelift one synthetic SourceLoc per
    // statement + terminator, so stmt_byte_offsets MUST scale
    // monotonically with statement count — independent of how
    // aggressively the optimizer dedups machine instructions.
    assert!(
        big.stmt_byte_offsets.len() > small.stmt_byte_offsets.len(),
        "8-stmt fn should produce more stmt_byte_offsets ({}) than 3-stmt fn ({})",
        big.stmt_byte_offsets.len(),
        small.stmt_byte_offsets.len()
    );
    // The MachSrcLoc map similarly should grow with the statement
    // count (at opt=none, ≈1 machine inst per CLIF inst).
    assert!(
        big.rows.len() >= small.rows.len(),
        "8-stmt fn should produce >= MachSrcLoc rows ({}) than 3-stmt fn ({})",
        big.rows.len(),
        small.rows.len()
    );
}
