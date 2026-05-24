//! Generic monomorphization (slice 8, A49).
//!
//! Slice-8 takes the simple route: every (fn, type-arg) tuple gets
//! its own specialized SIR function. Code bloat is accepted. The
//! `Monomorphizer` walks the program from `main`, discovers generic
//! call sites, specializes them, and rewrites the call graph.
//!
//! NOTE: The slice-3 typeck does not currently surface explicit
//! type-arg lists at every call site in the SIR we lower from. For
//! the slice-8 MVP, the monomorphizer is conservative — it leaves
//! `Param`-typed bindings alone (the interpreter is polymorphic
//! over them) and only specializes fns that have *no* unresolved
//! type parameters at the call boundary. The result is that
//! monomorphic programs (which the slice-8 codegen targets) work,
//! and generic programs fall through to interpreter via
//! `CodegenError::Unsupported`.

use sdust_sir::sir::{Function, Program, SirTy};

/// Returns true if `f` is generic (any param type or return uses
/// `SirTy::Param`).
pub fn is_generic(f: &Function) -> bool {
    fn uses_param(t: &SirTy) -> bool {
        match t {
            SirTy::Param(_) => true,
            SirTy::Tuple(xs) => xs.iter().any(uses_param),
            SirTy::Array { elem, .. } | SirTy::Ref { inner: elem, .. } | SirTy::RawPtr(elem) => {
                uses_param(elem)
            }
            SirTy::Fn { params, ret } => params.iter().any(uses_param) || uses_param(ret),
            SirTy::Adt(_, args) => args.iter().any(uses_param),
            _ => false,
        }
    }
    f.locals.iter().any(|l| uses_param(&l.ty)) || uses_param(&f.ret_ty)
}

pub struct Monomorphizer<'a> {
    pub prog: &'a Program,
}

impl<'a> Monomorphizer<'a> {
    pub fn new(prog: &'a Program) -> Self {
        Self { prog }
    }

    /// Slice-8 MVP: returns a (cloned) program whose non-generic fns
    /// are unchanged and whose generic fns are stripped. Real
    /// per-(fn, type-args) specialization is deferred to v0.2.
    pub fn run(&self) -> Program {
        let mut out = self.prog.clone();
        out.fns.retain(|f| !is_generic(f));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdust_hir::SourceSpan;
    use sdust_sir::sir::{Block, BlockId, Function, LocalDecl, LocalSource, Operand, SirFnId, Term};

    fn make_fn(name: &str, ret: SirTy) -> Function {
        Function {
            id: SirFnId(0),
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
                terminator: Term::Return(Operand::Const(sdust_sir::sir::Const::Unit)),
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
        let f = make_fn("id", SirTy::Param("T".into()));
        assert!(is_generic(&f));
    }

    #[test]
    fn concrete_fn_not_generic() {
        let f = make_fn("main", SirTy::Unit);
        assert!(!is_generic(&f));
    }

    #[test]
    fn mono_strips_generic_fns() {
        let mut p = Program::default();
        p.fns.push(make_fn("g", SirTy::Param("T".into())));
        p.fns.push(make_fn("main", SirTy::Unit));
        let m = Monomorphizer::new(&p).run();
        assert_eq!(m.fns.len(), 1);
        assert_eq!(m.fns[0].name, "main");
    }
}
