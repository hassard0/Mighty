//! Generic monomorphization (A49).
//!
//! v0.2 strategy:
//!
//! - **Walk the program from `main`** discovering reachable fns.
//! - For each generic fn that is reached, emit a *specialization* by
//!   substituting `IrTy::Param(T)` with a concrete representative
//!   type (slice-8's `i64`-pointer-sized fallback). This means we
//!   ship a single specialization per generic fn that compiles
//!   cleanly under the codegen's "everything's an i64 if I don't
//!   know better" policy.
//! - Mangled name: `<fn>__T0_T1_...` so future per-call-site
//!   specializations can coexist.
//!
//! Real per-(fn, type-args) specialization (a separate cached fn per
//! concrete tuple) is staged behind the upstream typeck propagating
//! explicit type-arg lists to call sites — see
//! `CODEGEN_V0_2_NOTES.md`. The current strategy keeps generic call
//! sites *callable* (no orphans) without requiring that propagation.

use mty_ir::ir::{Function, IrTy, Program};
use mty_types::IntKind;

/// Returns true if `f` is generic (any param type or return uses
/// `IrTy::Param`).
pub fn is_generic(f: &Function) -> bool {
    fn uses_param(t: &IrTy) -> bool {
        match t {
            IrTy::Param(_) => true,
            IrTy::Tuple(xs) => xs.iter().any(uses_param),
            IrTy::Array { elem, .. } | IrTy::Ref { inner: elem, .. } | IrTy::RawPtr(elem) => {
                uses_param(elem)
            }
            IrTy::Fn { params, ret } => params.iter().any(uses_param) || uses_param(ret),
            IrTy::Adt(_, args) => args.iter().any(uses_param),
            _ => false,
        }
    }
    f.locals.iter().any(|l| uses_param(&l.ty)) || uses_param(&f.ret_ty)
}

/// Replace any `IrTy::Param(_)` in `t` with the conservative
/// representative `IrTy::Int(I64)`. This matches what the cranelift
/// backend would lower a `Param`-typed binding to anyway, so the
/// specialized fn passes the verifier.
fn concretize(t: &IrTy) -> IrTy {
    match t {
        IrTy::Param(_) => IrTy::Int(IntKind::I64),
        IrTy::Tuple(xs) => IrTy::Tuple(xs.iter().map(concretize).collect()),
        IrTy::Array { elem, len } => IrTy::Array {
            elem: Box::new(concretize(elem)),
            len: *len,
        },
        IrTy::Ref { mutable, inner } => IrTy::Ref {
            mutable: *mutable,
            inner: Box::new(concretize(inner)),
        },
        IrTy::Fn { params, ret } => IrTy::Fn {
            params: params.iter().map(concretize).collect(),
            ret: Box::new(concretize(ret)),
        },
        IrTy::Adt(id, args) => IrTy::Adt(*id, args.iter().map(concretize).collect()),
        IrTy::RawPtr(inner) => IrTy::RawPtr(Box::new(concretize(inner))),
        other => other.clone(),
    }
}

/// Walk a function and substitute every Param-bearing type with its
/// concrete representative. Returns a new Function.
fn specialize(f: &Function, suffix: &str) -> Function {
    let mut g = f.clone();
    g.name = format!("{}__{}", f.name, suffix);
    g.ret_ty = concretize(&g.ret_ty);
    for local in g.locals.iter_mut() {
        local.ty = concretize(&local.ty);
    }
    g
}

pub struct Monomorphizer<'a> {
    pub prog: &'a Program,
}

impl<'a> Monomorphizer<'a> {
    pub fn new(prog: &'a Program) -> Self {
        Self { prog }
    }

    /// v0.2: keep generic fns in the program by emitting a *single*
    /// representative-typed specialization per generic fn (suffix
    /// `_mono`). The original generic fn is also retained so call
    /// sites that still reference it by `IrFnId` work (they'll
    /// dispatch to the specialized version because the codegen lowers
    /// Param-typed locals as i64 — the same shape as the specialization).
    ///
    /// **v0.8 honest measurement**: the per-fn `specialize` call is
    /// extremely cheap (clone + concretize walk). Benchmarks
    /// (`typeck_parallel`) show the std::thread::scope spin-up cost
    /// dominates up to 256 generic fns: parallel was ~8x slower at
    /// medium-32g, ~1.8x slower at large-256g. `run()` therefore
    /// stays on the sequential path; `run_parallel()` remains
    /// available for the future where per-fn typeck-per-instantiation
    /// makes the per-fn cost large enough to amortise the thread
    /// fan-out.
    ///
    /// **v0.10 re-bench** (Windows host, 4-worker fan-out, this
    /// commit's hardware — see `crates/mty-codegen-cranelift/benches/
    /// typeck_parallel.rs`):
    ///
    /// | fixture           | sequential | parallel | ratio |
    /// |-------------------|-----------:|---------:|------:|
    /// | small_4g          | 11 µs      | 12 µs    | 1.1x  |
    /// | medium_32g        | 57 µs      | 459 µs   | 8.0x  |
    /// | large_256g        | 377 µs     | 917 µs   | 2.4x  |
    /// | xlarge_1024g      | 1.42 ms    | 1.98 ms  | 1.4x  |
    /// | large_256g_fat†   | 4.00 ms    | 4.71 ms  | 1.2x  |
    ///
    /// † `_fat` = 64 locals per generic fn, modelling what `specialize`
    /// will look like once typeck-per-instantiation lands. Even at
    /// this size parallel still loses — the worker-pool spawn floor
    /// on Windows runs ~250 µs and we cannot hide it behind 16 µs of
    /// per-fn work even at 256 fns.
    ///
    /// **Verdict for v0.10**: the regression is fundamental, not a
    /// scheduler bug — per-fn `specialize` work is bound by `Function
    /// ::clone` + a single `concretize` walk that runs in ~1-2 µs
    /// (or ~16 µs in the fat variant). Even with the chunked
    /// partition (each worker batches `ceil(N/W)` fns) we cannot
    /// recover the ~250 µs thread-spawn floor on Windows. The cost
    /// model says parallel will start to win when *per-fn* work
    /// exceeds roughly 1 ms — that's the regime where typeck-per-
    /// instantiation lives (HIR walk, unification, constraint solve
    /// per call-site tuple), but it's well above anything mono does
    /// today. `run_parallel` therefore stays opt-in and `run()`
    /// dispatches to `run_sequential` for *all* current program
    /// sizes. The exposure remains documented + microbenched so a
    /// future caller can flip the default once the per-fn cost
    /// crosses the break-even point measured by `large_256g_fat`.
    pub fn run(&self) -> Program {
        self.run_sequential()
    }

    /// Sequential implementation. Always preferred today; see the
    /// `run()` docstring for the parallel-vs-sequential tradeoff.
    pub fn run_sequential(&self) -> Program {
        let mut out = self.prog.clone();
        let generics: Vec<usize> = self
            .prog
            .fns
            .iter()
            .enumerate()
            .filter(|(_, f)| is_generic(f))
            .map(|(i, _)| i)
            .collect();
        for i in generics {
            // Replace in-place: convert the generic fn to its
            // specialization. The SIR fn-id stays the same so call
            // sites continue to resolve.
            let spec = specialize(&self.prog.fns[i], "mono");
            out.fns[i] = spec;
        }
        out
    }

    /// v0.8 parallel mono.run: same semantics as `run()`, but
    /// distributes the `specialize` calls across worker threads using
    /// `std::thread::scope`. For programs with few generic fns this is
    /// no-op (the sequential path is taken); the threshold (>= 8
    /// generics) avoids the worker spin-up cost on small codebases.
    ///
    /// The compile-pipeline driver calls this in the `mty build` /
    /// `mty run` path; the determinism contract is unchanged because
    /// the per-fn output order is reassembled by index after the
    /// parallel collect.
    pub fn run_parallel(&self) -> Program {
        let mut out = self.prog.clone();
        let generics: Vec<usize> = self
            .prog
            .fns
            .iter()
            .enumerate()
            .filter(|(_, f)| is_generic(f))
            .map(|(i, _)| i)
            .collect();

        // Threshold: sequential path is cheaper below this many fns.
        if generics.len() < 8 {
            for i in generics {
                let spec = specialize(&self.prog.fns[i], "mono");
                out.fns[i] = spec;
            }
            return out;
        }

        // Choose a worker count: cap at 4 (mono is cheap per fn; more
        // threads is mostly contention) and the available
        // parallelism, whichever is smaller.
        let n_workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2)
            .clamp(1, 4);

        // Partition generics into n_workers roughly-equal chunks. The
        // chunks are contiguous slices of `generics`; each worker
        // outputs (index, specialized Function) pairs that we splice
        // back into `out` after the join.
        let chunks: Vec<Vec<usize>> = {
            let mut v: Vec<Vec<usize>> = (0..n_workers).map(|_| Vec::new()).collect();
            for (k, &idx) in generics.iter().enumerate() {
                v[k % n_workers].push(idx);
            }
            v
        };

        let prog = self.prog;
        let collected: Vec<(usize, Function)> = std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(n_workers);
            for chunk in &chunks {
                let chunk = chunk.clone();
                handles.push(s.spawn(move || {
                    let mut local: Vec<(usize, Function)> = Vec::with_capacity(chunk.len());
                    for idx in chunk {
                        let spec = specialize(&prog.fns[idx], "mono");
                        local.push((idx, spec));
                    }
                    local
                }));
            }
            let mut out = Vec::new();
            for h in handles {
                out.extend(h.join().expect("mono worker panicked"));
            }
            out
        });

        // Splice back deterministically: collected may be in
        // worker-completion order, but we assign by index so the
        // resulting Program is identical to the sequential path.
        for (idx, spec) in collected {
            out.fns[idx] = spec;
        }
        out
    }

    /// Lower-level: produce a fresh specialization of `f` named with
    /// `suffix`. Used by future per-call-site machinery; not invoked
    /// by `run()` yet (requires typeck type-arg propagation).
    pub fn specialize(&self, f: &Function, suffix: &str) -> Function {
        specialize(f, suffix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mty_hir::SourceSpan;
    use mty_ir::ir::{Block, BlockId, Function, IrFnId, LocalDecl, LocalSource, Operand, Term};

    fn make_fn(name: &str, ret: IrTy) -> Function {
        Function {
            id: IrFnId(0),
            name: name.into(),
            params: vec![],
            locals: vec![LocalDecl {
                name: "_0".into(),
                ty: ret.clone(),
                mutable: false,
                source: LocalSource::Return,
            }],
            blocks: vec![Block {
                id: BlockId(0),
                stmts: vec![],
                terminator: Term::Return(Operand::Const(mty_ir::ir::Const::Unit)),
            }],
            entry: BlockId(0),
            ret_ty: ret,
            effects: vec![],
            hir_fn: None,
            span: SourceSpan { start: 0, end: 0 },
        }
    }

    #[test]
    fn detects_generic_via_return() {
        let f = make_fn("id", IrTy::Param("T".into()));
        assert!(is_generic(&f));
    }

    #[test]
    fn concrete_fn_not_generic() {
        let f = make_fn("main", IrTy::Unit);
        assert!(!is_generic(&f));
    }

    #[test]
    fn mono_specializes_generic_fns_in_place() {
        let mut p = Program::default();
        p.fns.push(make_fn("g", IrTy::Param("T".into())));
        p.fns.push(make_fn("main", IrTy::Unit));
        let m = Monomorphizer::new(&p).run();
        // Both fns retained; the generic one is rewritten to a
        // concrete specialization with a mangled name.
        assert_eq!(m.fns.len(), 2);
        let g = m
            .fns
            .iter()
            .find(|f| f.name == "g__mono")
            .expect("specialized name");
        // Return type is no longer Param.
        assert!(!matches!(g.ret_ty, IrTy::Param(_)));
        let main = m
            .fns
            .iter()
            .find(|f| f.name == "main")
            .expect("main retained");
        assert!(matches!(main.ret_ty, IrTy::Unit));
    }

    #[test]
    fn specialize_renames_with_suffix() {
        let p = Program::default();
        let m = Monomorphizer::new(&p);
        let f = make_fn("foo", IrTy::Param("T".into()));
        let s = m.specialize(&f, "I32");
        assert_eq!(s.name, "foo__I32");
        assert!(!matches!(s.ret_ty, IrTy::Param(_)));
    }

    #[test]
    fn parallel_matches_sequential() {
        // Build a program with 32 generic fns + 8 concrete fns. The
        // parallel and sequential paths must produce identical
        // Programs (by fn name + ret_ty).
        let mut p = Program::default();
        for i in 0..32 {
            p.fns
                .push(make_fn(&format!("g{i}"), IrTy::Param("T".into())));
        }
        for i in 0..8 {
            p.fns.push(make_fn(&format!("c{i}"), IrTy::Unit));
        }
        let seq = Monomorphizer::new(&p).run_sequential();
        let par = Monomorphizer::new(&p).run_parallel();
        assert_eq!(seq.fns.len(), par.fns.len());
        for (a, b) in seq.fns.iter().zip(par.fns.iter()) {
            assert_eq!(a.name, b.name, "name mismatch (parallel != sequential)");
            assert_eq!(format!("{:?}", a.ret_ty), format!("{:?}", b.ret_ty));
        }
    }

    #[test]
    fn parallel_threshold_small_program() {
        // 4 generics → below threshold, parallel == sequential
        // (sequential path inside run_parallel).
        let mut p = Program::default();
        for i in 0..4 {
            p.fns
                .push(make_fn(&format!("g{i}"), IrTy::Param("T".into())));
        }
        let par = Monomorphizer::new(&p).run_parallel();
        assert_eq!(par.fns.len(), 4);
        for f in &par.fns {
            assert!(f.name.ends_with("__mono"));
        }
    }
}
