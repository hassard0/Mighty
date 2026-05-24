//! `Sendable` predicate for cross-agent message arguments.
//!
//! Reference: design doc §3.7. Slice-4 ruleset:
//!
//! - Copy types — Sendable
//! - Owned `String`/`Bytes` — Sendable
//! - Tuples / arrays of Sendable — Sendable
//! - Opaque ADTs — Sendable (BOLD slice-4 decision; tightens in slice 6)
//! - User struct/enum ADTs — Sendable iff every payload field is Sendable
//! - References / Fn / RawPtr — NOT Sendable
//! - Param / Var — Sendable (conservative permissive; slice-5 bounds tighten)

use mty_types::{AdtKind, DefMap, TyArena, TyData, TyId};

use crate::copy::is_copy;

pub fn is_sendable(ty: TyId, arena: &TyArena, defs: &DefMap) -> bool {
    is_sendable_inner(ty, arena, defs, &mut Vec::new())
}

fn is_sendable_inner(ty: TyId, arena: &TyArena, defs: &DefMap, visiting: &mut Vec<TyId>) -> bool {
    if visiting.contains(&ty) {
        // Cycle: treat as Sendable (conservative permissive for recursive types).
        return true;
    }
    match arena.get(ty) {
        // Disqualify references / raw pointers / fn pointers FIRST.
        TyData::Ref { .. } | TyData::RawPtr(_) | TyData::Fn { .. } => return false,
        // Tuples / arrays: recurse — don't short-circuit through is_copy
        // because a tuple of (Copy, &T) is Copy but NOT Sendable.
        TyData::Tuple(xs) => {
            visiting.push(ty);
            let xs = xs.clone();
            let r = xs
                .iter()
                .all(|t| is_sendable_inner(*t, arena, defs, visiting));
            visiting.pop();
            return r;
        }
        TyData::Array { elem, .. } => {
            return is_sendable_inner(*elem, arena, defs, visiting);
        }
        _ => {}
    }
    if is_copy(ty, arena, defs) {
        return true;
    }
    match arena.get(ty) {
        TyData::String | TyData::Bytes => true,
        TyData::Adt(id, _) => match defs.adt(*id).map(|a| a.kind) {
            Some(AdtKind::Opaque) | None => true,
            Some(AdtKind::Struct) | Some(AdtKind::Enum) => {
                visiting.push(ty);
                let adt = defs.adt(*id).cloned();
                let r = if let Some(adt) = adt {
                    adt.variants.iter().all(|v| {
                        v.fields
                            .iter()
                            .all(|f| is_sendable_inner(f.ty, arena, defs, visiting))
                    })
                } else {
                    true
                };
                visiting.pop();
                r
            }
        },
        TyData::Var(_) | TyData::Param(_) => true,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mty_types::{AdtDef, AdtKind, DefMap, FieldDef, TyArena, VariantDef};

    #[test]
    fn primitives_sendable() {
        let a = TyArena::new();
        let d = DefMap::default();
        assert!(is_sendable(a.i32, &a, &d));
        assert!(is_sendable(a.bool_, &a, &d));
        assert!(is_sendable(a.unit, &a, &d));
    }

    #[test]
    fn string_bytes_sendable() {
        let a = TyArena::new();
        let d = DefMap::default();
        assert!(is_sendable(a.string, &a, &d));
        assert!(is_sendable(a.bytes, &a, &d));
    }

    #[test]
    fn refs_not_sendable() {
        let mut a = TyArena::new();
        let d = DefMap::default();
        let r = a.ref_to(false, a.i32);
        let m = a.ref_to(true, a.i32);
        assert!(!is_sendable(r, &a, &d));
        assert!(!is_sendable(m, &a, &d));
    }

    #[test]
    fn raw_ptr_not_sendable() {
        let mut a = TyArena::new();
        let d = DefMap::default();
        let p = a.raw_ptr(a.u8);
        assert!(!is_sendable(p, &a, &d));
    }

    #[test]
    fn opaque_sendable() {
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
        assert!(is_sendable(t, &a, &d));
    }

    #[test]
    fn user_struct_sendable_if_fields_are() {
        let mut a = TyArena::new();
        let mut d = DefMap::default();
        // struct Snd { x: I32 } — sendable
        let aid = d.alloc_adt(AdtDef {
            name: "Snd".into(),
            kind: AdtKind::Struct,
            generics: vec![],
            param_ids: vec![],
            variants: vec![VariantDef {
                name: "Snd".into(),
                fields: vec![FieldDef {
                    name: Some("x".into()),
                    ty: a.i32,
                }],
            }],
        });
        let t = a.adt(aid, vec![]);
        assert!(is_sendable(t, &a, &d));

        // struct Holds { r: &I32 } — NOT sendable (slice 4: refs not sendable)
        let r = a.ref_to(false, a.i32);
        let bid = d.alloc_adt(AdtDef {
            name: "Holds".into(),
            kind: AdtKind::Struct,
            generics: vec![],
            param_ids: vec![],
            variants: vec![VariantDef {
                name: "Holds".into(),
                fields: vec![FieldDef {
                    name: Some("r".into()),
                    ty: r,
                }],
            }],
        });
        let bt = a.adt(bid, vec![]);
        assert!(!is_sendable(bt, &a, &d));
    }

    #[test]
    fn tuple_sendable_iff_all_parts() {
        let mut a = TyArena::new();
        let d = DefMap::default();
        let t1 = a.tuple(vec![a.i32, a.string]);
        assert!(is_sendable(t1, &a, &d));
        let r = a.ref_to(false, a.i32);
        let t2 = a.tuple(vec![a.i32, r]);
        assert!(!is_sendable(t2, &a, &d));
    }

    #[test]
    fn param_var_sendable_conservatively() {
        let mut a = TyArena::new();
        let d = DefMap::default();
        let p = a.param(mty_types::ParamId(0));
        let v = a.var(mty_types::TyVarId(0));
        assert!(is_sendable(p, &a, &d));
        assert!(is_sendable(v, &a, &d));
    }
}
