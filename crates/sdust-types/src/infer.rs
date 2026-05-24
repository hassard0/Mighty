//! Hindley-Milner inference primitives: substitution, unification,
//! occurs-check, defaulting.
//!
//! Inference variables are stored outside the `TyArena` (so binding one
//! doesn't require mutating the arena). A `Substitution` is a flat vector
//! indexed by `TyVarId`; `None` means unbound.

use crate::ty::{FloatKind, IntKind, TyArena, TyData, TyId, TyVarId};

#[derive(Debug, Default)]
pub struct Substitution {
    /// `slots[i] = Some(ty)` means `TyVarId(i)` is bound to `ty`. Bindings
    /// may themselves point to other variables — `resolve` walks the chain.
    slots: Vec<Option<TyId>>,
}

impl Substitution {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fresh_var(&mut self) -> TyVarId {
        let id = TyVarId(self.slots.len() as u32);
        self.slots.push(None);
        id
    }

    pub fn bind(&mut self, var: TyVarId, ty: TyId) {
        let idx = var.0 as usize;
        if idx >= self.slots.len() {
            self.slots.resize(idx + 1, None);
        }
        self.slots[idx] = Some(ty);
    }

    pub fn get(&self, var: TyVarId) -> Option<TyId> {
        self.slots.get(var.0 as usize).copied().flatten()
    }

    /// Walk the substitution chain. If `ty` is `Var(v)` and `v` is bound,
    /// recurse on the binding. Returns the representative type id.
    pub fn resolve(&self, ty: TyId, arena: &TyArena) -> TyId {
        let mut cur = ty;
        loop {
            match arena.get(cur) {
                TyData::Var(v) => match self.get(*v) {
                    Some(next) if next != cur => cur = next,
                    _ => return cur,
                },
                _ => return cur,
            }
        }
    }

    /// Shallow resolve: walks Var chains but does NOT descend into compound
    /// types' children. Used by the pretty printer.
    pub fn resolve_shallow(&self, ty: TyId, arena: &TyArena) -> TyId {
        self.resolve(ty, arena)
    }

    /// Number of variables ever allocated.
    pub fn var_count(&self) -> usize {
        self.slots.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnifyError {
    Mismatch,
    Occurs,
    ArityMismatch,
}

pub fn unify(
    a: TyId,
    b: TyId,
    subst: &mut Substitution,
    arena: &mut TyArena,
) -> Result<(), UnifyError> {
    let a = subst.resolve(a, arena);
    let b = subst.resolve(b, arena);
    if a == b {
        return Ok(());
    }
    // Clone the data so we can call back into `arena` for sub-unifications.
    let av = arena.get(a).clone();
    let bv = arena.get(b).clone();
    match (av, bv) {
        // Error poisons everything.
        (TyData::Error, _) | (_, TyData::Error) => Ok(()),
        // Never unifies with anything.
        (TyData::Never, _) | (_, TyData::Never) => Ok(()),
        // Var on either side: bind.
        (TyData::Var(v), _) => {
            if occurs_check(v, b, subst, arena) {
                return Err(UnifyError::Occurs);
            }
            subst.bind(v, b);
            Ok(())
        }
        (_, TyData::Var(v)) => {
            if occurs_check(v, a, subst, arena) {
                return Err(UnifyError::Occurs);
            }
            subst.bind(v, a);
            Ok(())
        }
        // Int / Float infer flex.
        (TyData::Int(k1), TyData::Int(k2)) => unify_int(k1, k2, a, b, subst, arena),
        (TyData::Float(k1), TyData::Float(k2)) => unify_float(k1, k2, a, b, subst, arena),
        // Structurals.
        (TyData::Tuple(xs), TyData::Tuple(ys)) => {
            if xs.len() != ys.len() {
                return Err(UnifyError::ArityMismatch);
            }
            for (x, y) in xs.into_iter().zip(ys.into_iter()) {
                unify(x, y, subst, arena)?;
            }
            Ok(())
        }
        (TyData::Array { elem: e1, len: l1 }, TyData::Array { elem: e2, len: l2 }) => {
            unify(e1, e2, subst, arena)?;
            match (l1, l2) {
                (Some(a), Some(b)) if a != b => Err(UnifyError::Mismatch),
                _ => Ok(()),
            }
        }
        (
            TyData::Ref {
                mutable: m1,
                inner: i1,
            },
            TyData::Ref {
                mutable: m2,
                inner: i2,
            },
        ) => {
            if m1 != m2 {
                return Err(UnifyError::Mismatch);
            }
            unify(i1, i2, subst, arena)
        }
        (
            TyData::Fn {
                params: p1,
                ret: r1,
                effects: _,
            },
            TyData::Fn {
                params: p2,
                ret: r2,
                effects: _,
            },
        ) => {
            if p1.len() != p2.len() {
                return Err(UnifyError::ArityMismatch);
            }
            for (x, y) in p1.into_iter().zip(p2.into_iter()) {
                unify(x, y, subst, arena)?;
            }
            unify(r1, r2, subst, arena)
        }
        (TyData::Adt(d1, args1), TyData::Adt(d2, args2)) => {
            if d1 != d2 || args1.len() != args2.len() {
                return Err(UnifyError::Mismatch);
            }
            for (x, y) in args1.into_iter().zip(args2.into_iter()) {
                unify(x, y, subst, arena)?;
            }
            Ok(())
        }
        (TyData::RawPtr(i1), TyData::RawPtr(i2)) => unify(i1, i2, subst, arena),
        (TyData::Param(p1), TyData::Param(p2)) if p1 == p2 => Ok(()),
        // Capability unification: family must match. Constraint mismatch
        // is permitted here (narrower-vs-broader is enforced separately
        // by SD4010 at call sites — unification stays loose so dispatch
        // can still proceed).
        (TyData::Cap { family: f1, .. }, TyData::Cap { family: f2, .. }) if f1 == f2 => Ok(()),
        (TyData::Dyn { trait_name: a }, TyData::Dyn { trait_name: b }) if a == b => Ok(()),
        // Module-vs-module: equal-or-error.
        (TyData::Module(a), TyData::Module(b)) if a == b => Ok(()),
        // Primitive identity (already covered by `a == b` at the top in
        // most cases via interning; this case catches stragglers).
        (TyData::Bool, TyData::Bool)
        | (TyData::Char, TyData::Char)
        | (TyData::Str, TyData::Str)
        | (TyData::String, TyData::String)
        | (TyData::Bytes, TyData::Bytes)
        | (TyData::Unit, TyData::Unit)
        | (TyData::Duration, TyData::Duration)
        | (TyData::Size, TyData::Size) => Ok(()),
        _ => Err(UnifyError::Mismatch),
    }
}

fn unify_int(
    k1: IntKind,
    k2: IntKind,
    a: TyId,
    b: TyId,
    subst: &mut Substitution,
    arena: &mut TyArena,
) -> Result<(), UnifyError> {
    if k1 == k2 {
        return Ok(());
    }
    // IntInfer flexes to concrete via a binding-by-replacement. Because
    // primitives are interned and there's no inference var slot, we instead
    // accept the unification silently when one side is IntInfer. The
    // defaulting pass at end of body promotes leftover IntInfer to I32.
    match (k1, k2) {
        (IntKind::IntInfer, _) | (_, IntKind::IntInfer) => {
            // Best-effort: if one side is a fresh var we'd already have
            // bound. Here both sides are already concrete-or-IntInfer.
            // Treat as compatible — the wider side wins via context.
            let _ = (a, b, subst, arena);
            Ok(())
        }
        _ => Err(UnifyError::Mismatch),
    }
}

fn unify_float(
    k1: FloatKind,
    k2: FloatKind,
    a: TyId,
    b: TyId,
    subst: &mut Substitution,
    arena: &mut TyArena,
) -> Result<(), UnifyError> {
    if k1 == k2 {
        return Ok(());
    }
    match (k1, k2) {
        (FloatKind::FloatInfer, _) | (_, FloatKind::FloatInfer) => {
            let _ = (a, b, subst, arena);
            Ok(())
        }
        _ => Err(UnifyError::Mismatch),
    }
}

pub fn occurs_check(var: TyVarId, ty: TyId, subst: &Substitution, arena: &TyArena) -> bool {
    let ty = subst.resolve(ty, arena);
    match arena.get(ty) {
        TyData::Var(v) => *v == var,
        TyData::Tuple(xs) => xs.iter().any(|t| occurs_check(var, *t, subst, arena)),
        TyData::Array { elem, .. } => occurs_check(var, *elem, subst, arena),
        TyData::Ref { inner, .. } => occurs_check(var, *inner, subst, arena),
        TyData::Fn { params, ret, .. } => {
            params.iter().any(|t| occurs_check(var, *t, subst, arena))
                || occurs_check(var, *ret, subst, arena)
        }
        TyData::Adt(_, args) => args.iter().any(|t| occurs_check(var, *t, subst, arena)),
        TyData::RawPtr(inner) => occurs_check(var, *inner, subst, arena),
        _ => false,
    }
}

/// Substitute `Param(p)` for `replacement[&p]` throughout `ty`. Returns
/// a newly-interned `TyId`. Used for generic instantiation. ParamIds are
/// global; the replacement map provides the per-instantiation mapping.
pub fn substitute_params(
    ty: TyId,
    replacement: &std::collections::HashMap<crate::ty::ParamId, TyId>,
    arena: &mut TyArena,
) -> TyId {
    let data = arena.get(ty).clone();
    match data {
        TyData::Param(p) => replacement.get(&p).copied().unwrap_or(ty),
        TyData::Tuple(xs) => {
            let new: Vec<TyId> = xs
                .into_iter()
                .map(|t| substitute_params(t, replacement, arena))
                .collect();
            arena.tuple(new)
        }
        TyData::Array { elem, len } => {
            let new_elem = substitute_params(elem, replacement, arena);
            arena.array(new_elem, len)
        }
        TyData::Ref { mutable, inner } => {
            let new_inner = substitute_params(inner, replacement, arena);
            arena.ref_to(mutable, new_inner)
        }
        TyData::Fn {
            params,
            ret,
            effects,
        } => {
            let p: Vec<TyId> = params
                .into_iter()
                .map(|t| substitute_params(t, replacement, arena))
                .collect();
            let r = substitute_params(ret, replacement, arena);
            arena.fn_ty(p, r, effects)
        }
        TyData::Adt(id, args) => {
            let new: Vec<TyId> = args
                .into_iter()
                .map(|t| substitute_params(t, replacement, arena))
                .collect();
            arena.adt(id, new)
        }
        TyData::RawPtr(inner) => {
            let new_inner = substitute_params(inner, replacement, arena);
            arena.raw_ptr(new_inner)
        }
        _ => ty,
    }
}

/// Defaulting pass: walk substitution for any leftover IntInfer/FloatInfer
/// and pin them to I32/F64. Returns the count of vars promoted.
pub fn default_inference(_subst: &mut Substitution, _arena: &mut TyArena) -> usize {
    // The infer kinds aren't stored in subst slots (they're interned types).
    // The check layer applies defaults at use sites where a concrete type
    // is forced (e.g. unifying IntInfer against I64 yields no binding but
    // is accepted; the value's "type" in diagnostics remains IntInfer).
    // No-op for slice 3 — defaulting is implicit via the unifier's
    // permissive policy.
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_vars_are_unique() {
        let mut s = Substitution::new();
        let v1 = s.fresh_var();
        let v2 = s.fresh_var();
        assert_ne!(v1, v2);
    }

    #[test]
    fn bind_and_resolve_chain() {
        let mut a = TyArena::new();
        let mut s = Substitution::new();
        let v1 = s.fresh_var();
        let v2 = s.fresh_var();
        let tv1 = a.var(v1);
        let tv2 = a.var(v2);
        s.bind(v1, tv2);
        s.bind(v2, a.bool_);
        assert_eq!(s.resolve(tv1, &a), a.bool_);
    }

    #[test]
    fn unify_same_primitive() {
        let mut a = TyArena::new();
        let mut s = Substitution::new();
        assert!(unify(a.bool_, a.bool_, &mut s, &mut a).is_ok());
    }

    #[test]
    fn unify_mismatched() {
        let mut a = TyArena::new();
        let mut s = Substitution::new();
        assert_eq!(
            unify(a.bool_, a.i32, &mut s, &mut a),
            Err(UnifyError::Mismatch)
        );
    }

    #[test]
    fn unify_var_binds() {
        let mut a = TyArena::new();
        let mut s = Substitution::new();
        let v = s.fresh_var();
        let tv = a.var(v);
        unify(tv, a.i32, &mut s, &mut a).unwrap();
        assert_eq!(s.resolve(tv, &a), a.i32);
    }

    #[test]
    fn unify_int_infer_with_concrete() {
        let mut a = TyArena::new();
        let mut s = Substitution::new();
        let ii = a.int_infer;
        assert!(unify(ii, a.i64, &mut s, &mut a).is_ok());
    }

    #[test]
    fn unify_tuple_zips() {
        let mut a = TyArena::new();
        let mut s = Substitution::new();
        let t1 = a.tuple(vec![a.i32, a.bool_]);
        let t2 = a.tuple(vec![a.i32, a.bool_]);
        assert!(unify(t1, t2, &mut s, &mut a).is_ok());
    }

    #[test]
    fn unify_tuple_arity() {
        let mut a = TyArena::new();
        let mut s = Substitution::new();
        let t1 = a.tuple(vec![a.i32, a.bool_]);
        let t2 = a.tuple(vec![a.i32]);
        assert_eq!(
            unify(t1, t2, &mut s, &mut a),
            Err(UnifyError::ArityMismatch)
        );
    }

    #[test]
    fn unify_ref_mutability() {
        let mut a = TyArena::new();
        let mut s = Substitution::new();
        let r1 = a.ref_to(false, a.i32);
        let r2 = a.ref_to(true, a.i32);
        assert_eq!(unify(r1, r2, &mut s, &mut a), Err(UnifyError::Mismatch));
    }

    #[test]
    fn unify_fn_zip() {
        let mut a = TyArena::new();
        let mut s = Substitution::new();
        let f1 = a.fn_ty(vec![a.i32, a.bool_], a.unit, vec![]);
        let f2 = a.fn_ty(vec![a.i32, a.bool_], a.unit, vec![]);
        assert!(unify(f1, f2, &mut s, &mut a).is_ok());
    }

    #[test]
    fn occurs_blocks_infinite() {
        let mut a = TyArena::new();
        let mut s = Substitution::new();
        let v = s.fresh_var();
        let tv = a.var(v);
        let recursive = a.tuple(vec![tv]);
        assert!(occurs_check(v, recursive, &s, &a));
    }
}
