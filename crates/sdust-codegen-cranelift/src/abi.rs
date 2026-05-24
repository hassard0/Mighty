//! Cranelift ABI / calling-convention helpers (spec §24.5 + A52).
//!
//! Slice-8 ships with 64-bit host targets only. The lowering uses
//! `SystemV` on linux/macos and `WindowsFastcall` on windows; both are
//! 64-bit conventions.

use cranelift_codegen::ir::{types as ct, AbiParam, Signature, Type};
use cranelift_codegen::isa::CallConv;
use sdust_sir::sir::SirTy;
use sdust_types::{FloatKind, IntKind};
use target_lexicon::{OperatingSystem, Triple};

/// Pick the right C-compatible calling convention for the host triple.
pub fn host_call_conv(triple: &Triple) -> CallConv {
    match triple.operating_system {
        OperatingSystem::Windows => CallConv::WindowsFastcall,
        _ => CallConv::SystemV,
    }
}

/// Lower a SIR type to the single cranelift type used in the
/// register-passing ABI. Aggregates flatten to a pointer for the
/// slice-8 calling convention (caller-allocated stack slot).
pub fn cl_ty_for(t: &SirTy) -> Type {
    match t {
        SirTy::Bool => ct::I8,
        SirTy::Char => ct::I32,
        SirTy::Int(k) => match k {
            IntKind::I8 | IntKind::U8 => ct::I8,
            IntKind::I16 | IntKind::U16 => ct::I16,
            IntKind::I32 | IntKind::U32 | IntKind::IntInfer => ct::I32,
            IntKind::I64 | IntKind::U64 => ct::I64,
            IntKind::ISize | IntKind::USize => ct::I64,
            IntKind::I128 | IntKind::U128 => ct::I128,
        },
        SirTy::Float(k) => match k {
            FloatKind::F32 => ct::F32,
            FloatKind::F64 | FloatKind::FloatInfer => ct::F64,
        },
        SirTy::Duration | SirTy::Size => ct::I64,
        // Pointer-sized for all aggregate / reference shapes — the
        // callee receives a pointer to a caller-allocated buffer.
        _ => ct::I64,
    }
}

/// Build a cranelift `Signature` for a SIR fn given its param + return
/// types. Aggregates are passed/returned by hidden pointer for
/// slice-8.
pub fn build_signature(triple: &Triple, params: &[SirTy], ret: &SirTy) -> Signature {
    let mut sig = Signature::new(host_call_conv(triple));
    if !matches!(ret, SirTy::Unit | SirTy::Never) {
        sig.returns.push(AbiParam::new(cl_ty_for(ret)));
    }
    for p in params {
        if matches!(p, SirTy::Unit | SirTy::Never) {
            continue;
        }
        sig.params.push(AbiParam::new(cl_ty_for(p)));
    }
    sig
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_codegen::ir::types as ct;

    #[test]
    fn host_call_conv_is_sane() {
        let _ = host_call_conv(&Triple::host());
    }

    #[test]
    fn int_widths_map_correctly() {
        assert_eq!(cl_ty_for(&SirTy::Int(IntKind::I8)), ct::I8);
        assert_eq!(cl_ty_for(&SirTy::Int(IntKind::I32)), ct::I32);
        assert_eq!(cl_ty_for(&SirTy::Int(IntKind::I64)), ct::I64);
    }

    #[test]
    fn unit_omits_return() {
        let sig = build_signature(&Triple::host(), &[], &SirTy::Unit);
        assert!(sig.returns.is_empty());
    }

    #[test]
    fn i64_return_present() {
        let sig = build_signature(&Triple::host(), &[], &SirTy::Int(IntKind::I64));
        assert_eq!(sig.returns.len(), 1);
        assert_eq!(sig.returns[0].value_type, ct::I64);
    }
}
