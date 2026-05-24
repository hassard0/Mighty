//! v0.3 (A65 — slice-3 hardening): the **Sendable** marker trait.
//!
//! A type is Sendable iff it can safely cross an agent boundary:
//!
//! - all `Copy` types are Sendable (primitives, tuples-of-Sendable, RawPtr,
//!   Cap, immutable shared refs to nothing — slice 5 only allows the first
//!   three through `is_field_copy`);
//! - any owned, Sized value that contains **no internal references** is
//!   Sendable (e.g. `String`, `Bytes`, plain `Vec[T] where T: Sendable`,
//!   and `derive(Sendable)` user structs);
//! - references (`&T`, `&mut T`), capability handles narrowed by path/host,
//!   and types that contain such references transitively are NOT Sendable.
//!
//! The `Owned` wrapper is implicit at slice-3: every value the user has not
//! explicitly borrowed counts as "owned". Slice 4 (borrow checker) is
//! responsible for ensuring the value at the send site is unborrowed; the
//! Sendable check here is the static *type-shape* gate.
//!
//! Returned from `sendable_reason` is a short human-readable explanation
//! suitable for MT3011's diagnostic note. `None` = type is Sendable.

use crate::defs::{AdtKind, DefMap};
use crate::ty::{TyArena, TyData, TyId};

/// Returns `None` if the type is Sendable. Otherwise returns a short reason
/// string for the MT3011 diagnostic. Operates on a substitution-resolved
/// `TyId` (callers should pass `subst.resolve(ty, arena)`).
pub fn sendable_reason(ty: TyId, arena: &TyArena, defs: &DefMap) -> Option<String> {
    sendable_reason_inner(ty, arena, defs, 0)
}

fn sendable_reason_inner(ty: TyId, arena: &TyArena, defs: &DefMap, depth: usize) -> Option<String> {
    // Bound: types nest at most a few levels in practice; cycle break.
    if depth > 64 {
        return None;
    }
    match arena.get(ty) {
        // --- Always-Sendable scalars -------------------------------------
        TyData::Bool
        | TyData::Int(_)
        | TyData::Float(_)
        | TyData::Char
        | TyData::Str
        | TyData::Unit
        | TyData::Never
        | TyData::Duration
        | TyData::Size
        | TyData::RawPtr(_)
        | TyData::Error => None,
        // --- Owned heap types (Sendable: owned + no internal refs) -------
        TyData::String | TyData::Bytes => None,
        // --- Module / fn pointers ----------------------------------------
        TyData::Module(_) => None,
        // `Fn` values currently carry no captured-borrow info; treat as
        // Sendable because the slice 5 type system doesn't yet express
        // closure captures. v0.3 keeps this conservative-permissive.
        TyData::Fn { .. } => None,
        // --- Compound: descend ------------------------------------------
        TyData::Tuple(xs) => {
            for (i, t) in xs.iter().enumerate() {
                if let Some(r) = sendable_reason_inner(*t, arena, defs, depth + 1) {
                    return Some(format!("tuple element {} is not Sendable: {}", i, r));
                }
            }
            None
        }
        TyData::Array { elem, .. } => sendable_reason_inner(*elem, arena, defs, depth + 1)
            .map(|r| format!("array element is not Sendable: {}", r)),
        TyData::Ref { mutable, .. } => Some(format!(
            "contains a `{}T` reference (references never cross agent boundaries)",
            if *mutable { "&mut " } else { "&" }
        )),
        TyData::Adt(aid, args) => {
            // 1. Capability values: `Net`, `Fs`, `Clock` etc. are runtime
            //    handles, not Sendable across agents (the receiving agent
            //    must own its own narrowed handle).
            if let Some(adt) = defs.adt(*aid) {
                if matches!(adt.name.as_str(), "Net" | "Fs" | "Clock" | "Dom" | "Model") {
                    return Some(format!(
                        "capability handle `{}` does not cross agent boundaries (use a typed \
                         message carrying the narrowed authority instead)",
                        adt.name
                    ));
                }
                // 2. AgentRef[T]: Sendable iff its target is — let users
                //    spread the actor topology by message passing.
                if adt.name == "AgentRef" {
                    if let Some(inner) = args.first().copied() {
                        return sendable_reason_inner(inner, arena, defs, depth + 1)
                            .map(|r| format!("AgentRef target is not Sendable: {}", r));
                    }
                    return None;
                }
                // 3. user-marked #[derive(Sendable)] structs short-circuit.
                if defs.user_sendable.contains(aid) {
                    return None;
                }
                // 4. Opaque ADTs (prelude `Url`, `Page`, ...): the prelude
                //    only registers immutable-data nominal types as opaque,
                //    so treat them as Sendable. This matches slice-3 intent
                //    (opaque means "we don't know the shape" -> permissive).
                if adt.kind == AdtKind::Opaque {
                    return None;
                }
                // 5. Struct / Enum: every field must be Sendable.
                //    Note: we walk the field's declared TyId directly. If a
                //    field is `Param(p)` (an unsubstituted generic), the
                //    Var/Param arm below is permissive — matches slice 3's
                //    intent of not over-rejecting at the marker-trait gate.
                let _ = args;
                for (vidx, v) in adt.variants.iter().enumerate() {
                    for (fidx, f) in v.fields.iter().enumerate() {
                        if let Some(r) = sendable_reason_inner(f.ty, arena, defs, depth + 1) {
                            let field_name = f
                                .name
                                .clone()
                                .unwrap_or_else(|| format!("variant-{}-field-{}", vidx, fidx));
                            return Some(format!(
                                "field `{}` of `{}` is not Sendable: {}",
                                field_name, adt.name, r
                            ));
                        }
                    }
                }
                None
            } else {
                None
            }
        }
        TyData::Var(_) | TyData::Param(_) => {
            // Generic / unbound: be permissive. Slice 3 fresh vars frequently
            // pin to Sendable types after defaulting; failing here would
            // regress canonical examples.
            None
        }
        TyData::Cap { family, .. } => Some(format!(
            "capability `{:?}` is not Sendable across agent boundaries",
            family
        )),
        TyData::Dyn { trait_name } => Some(format!(
            "`dyn {}` trait object is not Sendable (no static guarantee its impl is Sendable)",
            trait_name
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_are_sendable() {
        let a = TyArena::new();
        let d = DefMap::default();
        assert!(sendable_reason(a.bool_, &a, &d).is_none());
        assert!(sendable_reason(a.i32, &a, &d).is_none());
        assert!(sendable_reason(a.string, &a, &d).is_none());
    }

    #[test]
    fn ref_is_not_sendable() {
        let mut a = TyArena::new();
        let d = DefMap::default();
        let r = a.ref_to(false, a.i32);
        let reason = sendable_reason(r, &a, &d);
        assert!(reason.is_some(), "expected `&I32` to be non-Sendable");
        assert!(reason.unwrap().contains("reference"));
    }

    #[test]
    fn mut_ref_is_not_sendable() {
        let mut a = TyArena::new();
        let d = DefMap::default();
        let r = a.ref_to(true, a.string);
        let reason = sendable_reason(r, &a, &d);
        assert!(reason.is_some());
    }

    #[test]
    fn tuple_of_refs_explains_index() {
        let mut a = TyArena::new();
        let d = DefMap::default();
        let r = a.ref_to(false, a.i32);
        let t = a.tuple(vec![a.bool_, r]);
        let reason = sendable_reason(t, &a, &d).expect("non-Sendable");
        assert!(reason.starts_with("tuple element 1"), "got: {}", reason);
    }
}
