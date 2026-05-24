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

use mty_ir::ir::{Function, Program, IrTy};
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
    pub fn run(&self) -> Program {
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
    use mty_ir::ir::{
        Block, BlockId, Function, LocalDecl, LocalSource, Operand, IrFnId, Term,
    };

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
}
