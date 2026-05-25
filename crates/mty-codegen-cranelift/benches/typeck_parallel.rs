//! v0.8 microbench: parallel vs sequential monomorphization.
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

fn build_program(n_generics: usize, n_concrete: usize) -> Program {
    let mut p = Program::default();
    for i in 0..n_generics {
        p.fns.push(Function {
            id: IrFnId(0),
            name: format!("g{i}"),
            params: vec![],
            locals: vec![LocalDecl {
                name: "_0".into(),
                ty: IrTy::Param("T".into()),
                mutable: false,
                source: LocalSource::Return,
            }],
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
    // Three program sizes: small (below threshold), medium, large.
    for (label, generics, concrete) in [
        ("small_4g", 4, 16),
        ("medium_32g", 32, 64),
        ("large_256g", 256, 256),
    ] {
        let p = build_program(generics, concrete);

        let mut g = c.benchmark_group(format!("mono_{label}"));
        g.measurement_time(Duration::from_secs(4));

        g.bench_function("sequential", |b| {
            b.iter(|| {
                let m = Monomorphizer::new(&p);
                black_box(m.run_sequential());
            })
        });

        g.bench_function("parallel", |b| {
            b.iter(|| {
                let m = Monomorphizer::new(&p);
                black_box(m.run_parallel());
            })
        });

        g.finish();
    }
}

criterion_group!(benches, bench_mono);
criterion_main!(benches);
