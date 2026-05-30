//! Cranelift ABI / calling-convention helpers (spec §24.5 + A52).
//!
//! Slice-8 ships with 64-bit host targets only. The lowering uses
//! `SystemV` on linux/macos and `WindowsFastcall` on windows; both are
//! 64-bit conventions.
//!
//! v0.38 Track T3 — extern-c aggregate return support. Adds
//! [`build_extern_signature`] which models the SysV / Windows-fastcall
//! returned-struct ABI:
//!
//! * Aggregates ≤ 8 bytes ride a single integer return register (i64).
//!   The caller stores the i64 into a stack slot to recover the
//!   field bytes at their natural offsets.
//! * Aggregates 9..=16 bytes ride two integer return registers (i64 +
//!   i64) — SysV's "small composite" rule. Caller materialises both
//!   halves into the slot.
//! * Aggregates > 16 bytes are returned via a hidden first parameter
//!   (sret). Caller allocates the slot, passes its address, ignores
//!   the duplicated-address return value cranelift emits.
//!
//! The 16-byte cut-off mirrors the SysV ABI (System V AMD64 §3.2.3 /
//! "INTEGER, INTEGER" rule) and the Windows x64 ABI's pragmatic
//! treatment of small structs as up to 8 bytes (we still emit two i64
//! returns up to 16 on Windows; the linked C side has to opt into the
//! same packing, which mingw/clang-cl do by default). Real wgpu/winit
//! ABIs targeted by Mighty stay inside the 16-byte regime for the
//! common shapes (Point, Extent3d, Rect).

use cranelift_codegen::ir::{types as ct, AbiParam, ArgumentPurpose, Signature, Type};
use cranelift_codegen::isa::CallConv;
use mty_ir::ir::{AdtRef, IrTy};
use mty_types::{FloatKind, IntKind};
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
pub fn cl_ty_for(t: &IrTy) -> Type {
    match t {
        IrTy::Bool => ct::I8,
        IrTy::Char => ct::I32,
        IrTy::Int(k) => match k {
            IntKind::I8 | IntKind::U8 => ct::I8,
            IntKind::I16 | IntKind::U16 => ct::I16,
            IntKind::I32 | IntKind::U32 | IntKind::IntInfer => ct::I32,
            IntKind::I64 | IntKind::U64 => ct::I64,
            IntKind::ISize | IntKind::USize => ct::I64,
            IntKind::I128 | IntKind::U128 => ct::I128,
        },
        IrTy::Float(k) => match k {
            FloatKind::F32 => ct::F32,
            FloatKind::F64 | FloatKind::FloatInfer => ct::F64,
        },
        IrTy::Duration | IrTy::Size => ct::I64,
        // Pointer-sized for all aggregate / reference shapes — the
        // callee receives a pointer to a caller-allocated buffer.
        _ => ct::I64,
    }
}

/// Build a cranelift `Signature` for a SIR fn given its param + return
/// types. Aggregates are passed/returned by hidden pointer for
/// slice-8.
pub fn build_signature(triple: &Triple, params: &[IrTy], ret: &IrTy) -> Signature {
    let mut sig = Signature::new(host_call_conv(triple));
    if !matches!(ret, IrTy::Unit | IrTy::Never) {
        sig.returns.push(AbiParam::new(cl_ty_for(ret)));
    }
    for p in params {
        if matches!(p, IrTy::Unit | IrTy::Never) {
            continue;
        }
        sig.params.push(AbiParam::new(cl_ty_for(p)));
    }
    sig
}

/// v0.38 T2 — C ABI default argument promotions for the variadic
/// portion of a call (`...`). At a variadic call site C requires that:
///
/// * `float` is promoted to `double`
/// * any integer narrower than `int` is promoted to `int` (signed) or
///   `unsigned int` (unsigned), which in our model means I8/I16 → I32
///   and U8/U16 → U32
/// * `bool`/`char` are integer-promoted to I32
/// * pointers and wider scalars (I32/U32/I64/U64/F64/pointers) pass
///   through unchanged
///
/// Returns the cranelift IR type to use for the AbiParam at this
/// variadic slot, and whether the source was an unsigned int (so the
/// caller can pick `uextend` vs `sextend`). Aggregates collapse to a
/// pointer (caller-allocated buffer) the same way as fixed slots —
/// passing a struct through `...` is technically UB in C but matches
/// the existing fixed-slot convention.
pub fn cl_ty_for_variadic(t: &IrTy) -> (Type, bool) {
    use mty_types::{FloatKind, IntKind};
    match t {
        IrTy::Bool => (ct::I32, true),
        IrTy::Char => (ct::I32, true),
        IrTy::Int(k) => match k {
            IntKind::I8 | IntKind::I16 => (ct::I32, false),
            IntKind::U8 | IntKind::U16 => (ct::I32, true),
            IntKind::I32 | IntKind::IntInfer => (ct::I32, false),
            IntKind::U32 => (ct::I32, true),
            IntKind::I64 => (ct::I64, false),
            IntKind::U64 => (ct::I64, true),
            IntKind::ISize => (ct::I64, false),
            IntKind::USize => (ct::I64, true),
            IntKind::I128 => (ct::I128, false),
            IntKind::U128 => (ct::I128, true),
        },
        IrTy::Float(k) => match k {
            // The whole point of the variadic promotion: every float
            // arg is widened to F64 before the call.
            FloatKind::F32 | FloatKind::F64 | FloatKind::FloatInfer => (ct::F64, false),
        },
        IrTy::Duration | IrTy::Size => (ct::I64, true),
        // Pointer-sized for all pointer / aggregate shapes.
        _ => (ct::I64, true),
    }
}

/// v0.38 Track T3 — `Signature` builder that models the C-ABI returned-
/// struct calling convention. See the module doc for the size rules.
///
/// Returns a tuple of `(Signature, AggregateReturnKind)` so the call-site
/// lowering knows whether to allocate a slot and how many return values
/// to consume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateReturnKind {
    /// Scalar or Unit return — no aggregate handling needed.
    None,
    /// Aggregate ≤ 8 bytes — single i64 return register holding the
    /// packed bytes. Caller stores into a slot at offset 0.
    OneReg { size: u32 },
    /// Aggregate 9..=16 bytes — two i64 returns. Caller stores them
    /// into the slot at offsets 0 and 8.
    TwoReg { size: u32 },
    /// Aggregate > 16 bytes — sret hidden first param. Caller allocates
    /// the slot, passes its address as the first arg, ignores the
    /// duplicated address that cranelift emits as the return.
    Sret { size: u32 },
}

impl AggregateReturnKind {
    /// True if the call site needs to allocate a stack slot for the
    /// return value.
    pub fn needs_slot(self) -> bool {
        !matches!(self, AggregateReturnKind::None)
    }
}

/// Compute the aggregate-return kind for a SIR return type, given the
/// program's ADT catalog. Non-aggregate / Unit / Never types map to
/// `AggregateReturnKind::None`.
pub fn classify_aggregate_return(ret: &IrTy, adts: &[AdtRef]) -> AggregateReturnKind {
    if matches!(ret, IrTy::Unit | IrTy::Never) {
        return AggregateReturnKind::None;
    }
    if !crate::aggregate::is_aggregate(ret) {
        return AggregateReturnKind::None;
    }
    let size = crate::aggregate::type_size(ret, adts);
    if size <= 8 {
        AggregateReturnKind::OneReg { size }
    } else if size <= 16 {
        AggregateReturnKind::TwoReg { size }
    } else {
        AggregateReturnKind::Sret { size }
    }
}

/// Build an extern-c call signature with returned-struct support.
/// Returns the `Signature` paired with the classification so the
/// call-site lowerer can drive slot setup, sret arg insertion, and
/// per-register result store.
pub fn build_extern_signature(
    triple: &Triple,
    params: &[IrTy],
    ret: &IrTy,
    adts: &[AdtRef],
) -> (Signature, AggregateReturnKind) {
    let kind = classify_aggregate_return(ret, adts);
    let mut sig = Signature::new(host_call_conv(triple));
    match kind {
        AggregateReturnKind::None => {
            if !matches!(ret, IrTy::Unit | IrTy::Never) {
                sig.returns.push(AbiParam::new(cl_ty_for(ret)));
            }
        }
        AggregateReturnKind::OneReg { .. } => {
            sig.returns.push(AbiParam::new(ct::I64));
        }
        AggregateReturnKind::TwoReg { .. } => {
            sig.returns.push(AbiParam::new(ct::I64));
            sig.returns.push(AbiParam::new(ct::I64));
        }
        AggregateReturnKind::Sret { .. } => {
            // sret hidden first param marked with the StructReturn
            // purpose so cranelift's verifier accepts it as the
            // implicit caller-allocated buffer pointer.
            //
            // cranelift's machinst layer rejects any explicit return
            // values when a param carries `ArgumentPurpose::StructReturn`
            // (the verifier expects sret to be the *only* output
            // channel). On real SysV the callee typically also echoes
            // the buffer pointer through RAX, but cranelift's calling
            // conventions model that internally — we leave `returns`
            // empty here and rely on the caller to use the slot
            // address it allocated, not the (absent) return value.
            sig.params
                .push(AbiParam::special(ct::I64, ArgumentPurpose::StructReturn));
        }
    }
    for p in params {
        if matches!(p, IrTy::Unit | IrTy::Never) {
            continue;
        }
        sig.params.push(AbiParam::new(cl_ty_for(p)));
    }
    (sig, kind)
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
        assert_eq!(cl_ty_for(&IrTy::Int(IntKind::I8)), ct::I8);
        assert_eq!(cl_ty_for(&IrTy::Int(IntKind::I32)), ct::I32);
        assert_eq!(cl_ty_for(&IrTy::Int(IntKind::I64)), ct::I64);
    }

    #[test]
    fn unit_omits_return() {
        let sig = build_signature(&Triple::host(), &[], &IrTy::Unit);
        assert!(sig.returns.is_empty());
    }

    #[test]
    fn i64_return_present() {
        let sig = build_signature(&Triple::host(), &[], &IrTy::Int(IntKind::I64));
        assert_eq!(sig.returns.len(), 1);
        assert_eq!(sig.returns[0].value_type, ct::I64);
    }

    #[test]
    fn variadic_promotes_small_signed_ints_to_i32() {
        // C ABI: signed char / short -> int. Our model: I8/I16 -> I32.
        assert_eq!(
            cl_ty_for_variadic(&IrTy::Int(IntKind::I8)),
            (ct::I32, false)
        );
        assert_eq!(
            cl_ty_for_variadic(&IrTy::Int(IntKind::I16)),
            (ct::I32, false)
        );
    }

    #[test]
    fn variadic_promotes_small_unsigned_ints_to_i32_unsigned() {
        // C ABI: unsigned char / unsigned short -> unsigned int.
        assert_eq!(cl_ty_for_variadic(&IrTy::Int(IntKind::U8)), (ct::I32, true));
        assert_eq!(
            cl_ty_for_variadic(&IrTy::Int(IntKind::U16)),
            (ct::I32, true)
        );
    }

    #[test]
    fn variadic_promotes_f32_to_f64() {
        use mty_types::FloatKind;
        assert_eq!(
            cl_ty_for_variadic(&IrTy::Float(FloatKind::F32)),
            (ct::F64, false)
        );
        assert_eq!(
            cl_ty_for_variadic(&IrTy::Float(FloatKind::F64)),
            (ct::F64, false)
        );
    }

    #[test]
    fn variadic_passes_wider_ints_unchanged() {
        assert_eq!(
            cl_ty_for_variadic(&IrTy::Int(IntKind::I32)),
            (ct::I32, false)
        );
        assert_eq!(
            cl_ty_for_variadic(&IrTy::Int(IntKind::U32)),
            (ct::I32, true)
        );
        assert_eq!(
            cl_ty_for_variadic(&IrTy::Int(IntKind::I64)),
            (ct::I64, false)
        );
        assert_eq!(
            cl_ty_for_variadic(&IrTy::Int(IntKind::U64)),
            (ct::I64, true)
        );
    }

    #[test]
    fn variadic_bool_and_char_promote_to_i32() {
        assert_eq!(cl_ty_for_variadic(&IrTy::Bool), (ct::I32, true));
        assert_eq!(cl_ty_for_variadic(&IrTy::Char), (ct::I32, true));
    }
}
