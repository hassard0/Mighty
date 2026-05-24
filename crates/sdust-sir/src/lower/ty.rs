//! HIR `TyId` → SIR `SirTy` translation.

use crate::sir::*;
use sdust_types::{TyArena, TyData, TyId};

pub fn lower_ty(ty: TyId, arena: &TyArena) -> SirTy {
    match arena.get(ty) {
        TyData::Bool => SirTy::Bool,
        TyData::Int(k) => SirTy::Int(*k),
        TyData::Float(k) => SirTy::Float(*k),
        TyData::Char => SirTy::Char,
        TyData::Str => SirTy::Str,
        TyData::String => SirTy::String,
        TyData::Bytes => SirTy::Bytes,
        TyData::Unit => SirTy::Unit,
        TyData::Never => SirTy::Never,
        TyData::Duration => SirTy::Duration,
        TyData::Size => SirTy::Size,
        TyData::Tuple(xs) => SirTy::Tuple(xs.iter().map(|x| lower_ty(*x, arena)).collect()),
        TyData::Array { elem, len } => SirTy::Array {
            elem: Box::new(lower_ty(*elem, arena)),
            len: *len,
        },
        TyData::Ref { mutable, inner } => SirTy::Ref {
            mutable: *mutable,
            inner: Box::new(lower_ty(*inner, arena)),
        },
        TyData::Fn { params, ret, .. } => SirTy::Fn {
            params: params.iter().map(|p| lower_ty(*p, arena)).collect(),
            ret: Box::new(lower_ty(*ret, arena)),
        },
        TyData::Adt(id, args) => {
            SirTy::Adt(*id, args.iter().map(|a| lower_ty(*a, arena)).collect())
        }
        TyData::Var(_) => SirTy::Error,
        TyData::Param(p) => SirTy::Param(format!("T{}", p.0)),
        TyData::RawPtr(inner) => SirTy::RawPtr(Box::new(lower_ty(*inner, arena))),
        TyData::Module(n) => SirTy::Module(n.clone()),
        TyData::Cap { family, constraint } => SirTy::Cap {
            family: family.clone(),
            constraint: constraint.clone(),
        },
        TyData::Dyn { trait_name } => SirTy::Dyn(trait_name.clone()),
        TyData::Error => SirTy::Error,
    }
}
