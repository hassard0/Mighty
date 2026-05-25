//! v0.8 microbench: parallel vs sequential monomorphization.
//! v0.10 polish: added `xlarge_1024g` to validate the regression
//! holds for programs an order of magnitude larger than any real
//! codebase we expect, plus a `fat` variant where each generic fn
//! has a much wider local table so per-fn `specialize` cost grows
//! beyond the thread-spawn floor. The `fat` numbers tell us
//! roughly when parallel WILL become profitable.
//!
//! The mono.run path specializes generic fns; for any program with
//! enough generics it dominates pre-codegen wall time. The parallel
//! variant fans the specialize call out across worker threads.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mty_codegen_cranelift::Monomorphizer;
use mty_hir::SourceSpan;
use mty_ir::ir::{
    Block, BlockId, Const, Function, IrFnId, IrTy, LocalDecl, LocalSource, Operand, Program, Term,
};
use mty_types::IntKind;
use std::time::Duration;

/// Build a program with `n_generics` generic fns and `n_concrete`
/// concrete fns. When `wide_locals` > 1, each fn gets that many
/// locals so the `specialize` cost (per-local concretize walk)
/// scales linearly — useful to simulate what a real typeck pass
/// will cost once explicit type-arg propagation lands.
fn build_program(n_generics: usize, n_concrete: usize, wide_locals: usize) -> Program {
    let mut p = Program::default();
    for i in 0..n_generics {
        let mut locals = vec![LocalDecl {
            name: "_0".into(),
            ty: IrTy::Param("T".into()),
            mutable: false,
            source: LocalSource::Return,
        }];
        for k in 1..wide_locals {
            locals.push(LocalDecl {
                name: format!("_{k}"),
                ty: IrTy::Param("T".into()),
                mutable: false,
                source: LocalSource::Temp,
            });
        }
        p.fns.push(Function {
            id: IrFnId(0),
            name: format!("g{i}"),
            params: vec![],
            locals,
            blocks: vec![Block {
                id: BlockId(0),
                stmts: vec![],
                terminator: Term::Return(Operand::Const(Const::Unit)),
            }],
            entry: BlockId(0),
            ret_ty: IrTy::Param("T".into()),
            effects: vec![],
            hir_fn: None,
            span: SourceSpan { start: 0, end: 0 },
        });
    }
    for i in 0..n_concrete {
        p.fns.push(Function {
            id: IrFnId(0),
            name: format!("c{i}"),
            params: vec![],
            locals: vec![LocalDecl {
                name: "_0".into(),
                ty: IrTy::Int(IntKind::I64),
                mutable: false,
                source: LocalSource::Return,
            }],
            blocks: vec![Block {
                id: BlockId(0),
                stmts: vec![],
                terminator: Term::Return(Operand::Const(Const::Unit)),
            }],
            entry: BlockId(0),
            ret_ty: IrTy::Int(IntKind::I64),
            effects: vec![],
            hir_fn: None,
            span: SourceSpan { start: 0, end: 0 },
        });
    }
    p
}

fn bench_mono(c: &mut Criterion) {
    // Five program sizes:
    //   - small / medium / large: the v0.8 regression set
    //   - xlarge_1024g: validates the regression at "no real
    //     codebase we expect to see" scale
    //   - large_256g_fat: same generic count, but each fn has 64
    //     locals so per-fn `specialize` cost dominates thread setup.
    //     This tells us roughly when parallel will start to win.
    for (label, generics, concrete, wide_locals) in [
        ("small_4g", 4, 16, 1),
        ("medium_32g", 32, 64, 1),
        ("large_256g", 256, 256, 1),
        ("xlarge_1024g", 1024, 256, 1),
        ("large_256g_fat", 256, 256, 64),
    ] {
        let p = build_program(generics, concrete, wide_locals);

        let mut g = c.benchmark_group(format!("mono_{label}"));
        g.measurement_time(Duration::from_secs(4));

        g.bench_function("sequential", |b| {
            b.iter(|| {
                let m = Monomorphizer::new(&p);
                black_box(m.run_sequential());
            });
        });

        g.bench_function("parallel", |b| {
            b.iter(|| {
                let m = Monomorphizer::new(&p);
                black_box(m.run_parallel());
            });
        });

        g.finish();
    }
}

criterion_group!(benches, bench_mono);
criterion_main!(benches);
