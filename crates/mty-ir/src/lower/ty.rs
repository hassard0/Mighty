//! HIR `TyId` → SIR `IrTy` translation.

use crate::ir::*;
use mty_types::{TyArena, TyData, TyId};

pub fn lower_ty(ty: TyId, arena: &TyArena) -> IrTy {
    match arena.get(ty) {
        TyData::Bool => IrTy::Bool,
        TyData::Int(k) => IrTy::Int(*k),
        TyData::Float(k) => IrTy::Float(*k),
        TyData::Char => IrTy::Char,
        TyData::Str => IrTy::Str,
        TyData::String => IrTy::String,
        TyData::Bytes => IrTy::Bytes,
        TyData::Unit => IrTy::Unit,
        TyData::Never => IrTy::Never,
        TyData::Duration => IrTy::Duration,
        TyData::Size => IrTy::Size,
        TyData::Tuple(xs) => IrTy::Tuple(xs.iter().map(|x| lower_ty(*x, arena)).collect()),
        TyData::Array { elem, len } => IrTy::Array {
            elem: Box::new(lower_ty(*elem, arena)),
            len: *len,
        },
        TyData::Ref { mutable, inner } => IrTy::Ref {
            mutable: *mutable,
            inner: Box::new(lower_ty(*inner, arena)),
        },
        TyData::Fn { params, ret, .. } => IrTy::Fn {
            params: params.iter().map(|p| lower_ty(*p, arena)).collect(),
            ret: Box::new(lower_ty(*ret, arena)),
        },
        TyData::Adt(id, args) => IrTy::Adt(*id, args.iter().map(|a| lower_ty(*a, arena)).collect()),
        TyData::Var(_) => IrTy::Error,
        TyData::Param(p) => IrTy::Param(format!("T{}", p.0)),
        TyData::RawPtr(inner) => IrTy::RawPtr(Box::new(lower_ty(*inner, arena))),
        TyData::Module(n) => IrTy::Module(n.clone()),
        TyData::Cap { family, constraint } => IrTy::Cap {
            family: family.clone(),
            constraint: constraint.clone(),
        },
        TyData::Dyn { trait_name } => IrTy::Dyn(trait_name.clone()),
        TyData::Error => IrTy::Error,
    }
}
