//! `Copy` predicate. Slice 4 hardcodes Copy-ness rather than driving it
//! from a `derive(Copy)` annotation (which arrives in slice 5).
//!
//! Reference: design doc §3.6.
//!
//! - Primitives (Bool, Int*, Float*, Char, Unit, Duration, Size) — Copy
//! - Shared references `&T` — Copy
//! - Mutable references `&mut T` — NOT Copy
//! - Raw pointers `*T` — Copy
//! - `Str` (string slice) — Copy. `String`/`Bytes` (heap-owning) — NOT.
//! - Tuples — Copy iff every element is Copy
//! - Fixed arrays — Copy iff element is Copy
//! - Fn pointers — Copy
//! - **Opaque** ADTs (prelude types like `Url`, `Page`, `Logger`) — Copy
//!   (BOLD slice-4 decision to keep examples compiling; tightens to per-type
//!   Copy bound in slice 5)
//! - User-declared structs/enums — NOT Copy
//! - `Param`/`Var` — NOT Copy (conservative)
//! - `Module`, `Never`, `Error` — Copy (degenerate)

use sdust_types::{AdtKind, DefMap, FloatKind, IntKind, TyArena, TyData, TyId};

pub fn is_copy(ty: TyId, arena: &TyArena, defs: &DefMap) -> bool {
    match arena.get(ty) {
        TyData::Bool
        | TyData::Char
        | TyData::Str
        | TyData::Unit
        | TyData::Never
        | TyData::Duration
        | TyData::Size
        | TyData::Error
        | TyData::Module(_) => true,
        TyData::Int(IntKind::IntInfer) | TyData::Float(FloatKind::FloatInfer) => true,
        TyData::Int(_) | TyData::Float(_) => true,
        TyData::String | TyData::Bytes => false,
        TyData::RawPtr(_) => true,
        TyData::Ref { mutable, .. } => !*mutable,
        TyData::Fn { .. } => true,
        TyData::Tuple(xs) => xs.iter().all(|t| is_copy(*t, arena, defs)),
        TyData::Array { elem, .. } => is_copy(*elem, arena, defs),
        TyData::Adt(id, _) => match defs.adt(*id).map(|a| a.kind) {
            Some(AdtKind::Opaque) => true,
            Some(AdtKind::Struct) | Some(AdtKind::Enum) => false,
            None => true,
        },
        TyData::Var(_) | TyData::Param(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdust_types::{AdtDef, AdtKind, DefMap, TyArena, VariantDef};

    #[test]
    fn primitives_are_copy() {
        let a = TyArena::new();
        let d = DefMap::default();
        assert!(is_copy(a.bool_, &a, &d));
        assert!(is_copy(a.i32, &a, &d));
        assert!(is_copy(a.f64, &a, &d));
        assert!(is_copy(a.char_, &a, &d));
        assert!(is_copy(a.str_, &a, &d));
        assert!(is_copy(a.unit, &a, &d));
    }

    #[test]
    fn heap_owning_not_copy() {
        let a = TyArena::new();
        let d = DefMap::default();
        assert!(!is_copy(a.string, &a, &d));
        assert!(!is_copy(a.bytes, &a, &d));
    }

    #[test]
    fn shared_ref_is_copy_mut_is_not() {
        let mut a = TyArena::new();
        let d = DefMap::default();
        let r = a.ref_to(false, a.i32);
        let m = a.ref_to(true, a.i32);
        assert!(is_copy(r, &a, &d));
        assert!(!is_copy(m, &a, &d));
    }

    #[test]
    fn tuple_of_copy_is_copy() {
        let mut a = TyArena::new();
        let d = DefMap::default();
        let t1 = a.tuple(vec![a.i32, a.bool_]);
        assert!(is_copy(t1, &a, &d));
        let t2 = a.tuple(vec![a.i32, a.string]);
        assert!(!is_copy(t2, &a, &d));
    }

    #[test]
    fn array_inherits_copy() {
        let mut a = TyArena::new();
        let d = DefMap::default();
        let arr_copy = a.array(a.i32, Some(4));
        let arr_string = a.array(a.string, Some(4));
        assert!(is_copy(arr_copy, &a, &d));
        assert!(!is_copy(arr_string, &a, &d));
    }

    #[test]
    fn user_struct_not_copy() {
        let mut a = TyArena::new();
        let mut d = DefMap::default();
        let aid = d.alloc_adt(AdtDef {
            name: "User".into(),
            kind: AdtKind::Struct,
            generics: vec![],
            param_ids: vec![],
            variants: vec![VariantDef {
                name: "User".into(),
                fields: vec![],
            }],
        });
        let t = a.adt(aid, vec![]);
        assert!(!is_copy(t, &a, &d));
    }

    #[test]
    fn opaque_adt_is_copy() {
        let mut a = TyArena::new();
        let mut d = DefMap::default();
        let aid = d.alloc_adt(AdtDef {
            name: "Url".into(),
            kind: AdtKind::Opaque,
            generics: vec![],
            param_ids: vec![],
            variants: vec![],
        });
        let t = a.adt(aid, vec![]);
        assert!(is_copy(t, &a, &d));
    }
}
