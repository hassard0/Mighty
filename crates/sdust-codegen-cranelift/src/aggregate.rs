//! Aggregate value support for the cranelift backend (v0.2).
//!
//! Slice-8 only handled scalar locals. v0.2 lifts ADTs, tuples, and
//! arrays by giving each non-scalar local a **stack slot**. The
//! cranelift `Variable` for the local then carries the slot's *address*
//! (i64), and reads/writes go through `load.ty offset` / `store.ty
//! offset` against that address.
//!
//! Enum layout:
//! - 4-byte tag at offset 0
//! - payload starts at `align_up(4, payload_align)`
//! - within the payload, fields lay out sequentially with natural
//!   alignment (see [`crate::layout::layout_with_adts`]).
//!
//! Struct layout: same as a one-variant enum *without* the tag word.
//!
//! Tuple layout: same as a struct with anonymous fields, in source order.
//!
//! Array layout: `len` consecutive elements, each `elem.size` bytes.
//!
//! This module is deliberately conservative — no field reordering, no
//! niche optimization, no sret tuning. The goal is correctness for the
//! v0.2 examples; future slices can specialize.

use crate::layout::{align_up, layout_with_adts, Layout, PTR_BYTES};
use cranelift_codegen::ir::types as ct;
use cranelift_codegen::ir::Type as ClType;
use sdust_sir::sir::{AdtRef, SirTy};
use sdust_types::IntKind;

/// Offset of the enum tag word (always 0).
pub const TAG_OFFSET: u32 = 0;
/// Size of the enum tag (u32).
pub const TAG_SIZE: u32 = 4;

/// True if a SIR type should be passed by *pointer* (i64 address) instead
/// of by value. Anything aggregate, plus strings (`(ptr, len)`).
pub fn is_aggregate(t: &SirTy) -> bool {
    matches!(t, SirTy::Tuple(_) | SirTy::Array { .. } | SirTy::Adt(_, _))
}

/// Compute layout for a SIR type (delegates to `layout_with_adts` for
/// aggregates; calls into primitive_layout for scalars).
pub fn type_layout(t: &SirTy, adts: &[AdtRef]) -> Layout {
    layout_with_adts(t, adts)
}

/// Returns Some(offset, size) for variant `v` field `f` in `adt`. The
/// offset is **relative to the start of the aggregate** (not the
/// payload). Returns None if the variant or field index is OOB.
pub fn variant_field_offset(
    adt: &AdtRef,
    variant: usize,
    field: usize,
    adts: &[AdtRef],
) -> Option<(u32, Layout)> {
    let v = adt.variants.get(variant)?;
    let f_ty = &v.fields.get(field)?.ty;
    let f_layout = type_layout(f_ty, adts);
    // payload offset = align_up(tag end, max_payload_align) when the ADT
    // is an enum (>1 variant). For structs, the "payload" starts at 0.
    let payload_start = if adt.variants.len() > 1 {
        let pal = max_payload_align(adt, adts);
        align_up(TAG_SIZE, pal)
    } else {
        0
    };
    // Within the variant, fields lay out sequentially with natural
    // alignment.
    let mut off: u32 = 0;
    let mut align: u32 = 1;
    for fi in 0..field {
        let fty = &v.fields[fi].ty;
        let l = type_layout(fty, adts);
        align = align.max(l.align);
        off = align_up(off, l.align);
        off += l.size;
    }
    off = align_up(off, f_layout.align);
    let _ = align;
    Some((payload_start + off, f_layout))
}

/// Field offset for a struct (single-variant ADT).
pub fn struct_field_offset(adt: &AdtRef, field: usize, adts: &[AdtRef]) -> Option<(u32, Layout)> {
    variant_field_offset(adt, 0, field, adts)
}

/// Tuple element offset.
pub fn tuple_offset(elems: &[SirTy], idx: usize, adts: &[AdtRef]) -> Option<(u32, Layout)> {
    if idx >= elems.len() {
        return None;
    }
    let elem_ty = &elems[idx];
    let elem_layout = type_layout(elem_ty, adts);
    let mut off: u32 = 0;
    for prev in &elems[..idx] {
        let l = type_layout(prev, adts);
        off = align_up(off, l.align);
        off += l.size;
    }
    off = align_up(off, elem_layout.align);
    Some((off, elem_layout))
}

fn max_payload_align(adt: &AdtRef, adts: &[AdtRef]) -> u32 {
    let mut a = 1;
    for v in &adt.variants {
        for f in &v.fields {
            let l = type_layout(&f.ty, adts);
            a = a.max(l.align);
        }
    }
    a
}

/// Determine the load/store cranelift type for an N-byte scalar slot.
/// 1 → i8, 2 → i16, 4 → i32, 8 → i64. For pointer-sized slots we use
/// i64. Aggregates inside aggregates raise None (caller handles via
/// memcpy).
pub fn load_store_ty(size: u32) -> Option<ClType> {
    Some(match size {
        1 => ct::I8,
        2 => ct::I16,
        4 => ct::I32,
        8 => ct::I64,
        _ => return None,
    })
}

/// Pick the load/store width to use for a field of the given SIR type.
/// For scalar fields we use the exact width; for aggregate fields we
/// return None (the caller will issue a memcpy). Best-effort: poisoned
/// `SirTy::Error` types are treated as i64 to keep the lowerer total
/// when upstream typeck didn't resolve the binding.
pub fn field_load_ty(t: &SirTy) -> Option<ClType> {
    Some(match t {
        SirTy::Bool => ct::I8,
        SirTy::Char => ct::I32,
        SirTy::Int(k) => match k {
            IntKind::I8 | IntKind::U8 => ct::I8,
            IntKind::I16 | IntKind::U16 => ct::I16,
            IntKind::I32 | IntKind::U32 | IntKind::IntInfer => ct::I32,
            IntKind::I64 | IntKind::U64 | IntKind::ISize | IntKind::USize => ct::I64,
            IntKind::I128 | IntKind::U128 => return None,
        },
        SirTy::Float(k) => match k {
            sdust_types::FloatKind::F32 => ct::F32,
            sdust_types::FloatKind::F64 | sdust_types::FloatKind::FloatInfer => ct::F64,
        },
        SirTy::Duration | SirTy::Size => ct::I64,
        SirTy::Str | SirTy::String | SirTy::Bytes => return None, // (ptr, len) pair
        SirTy::Ref { .. } | SirTy::RawPtr(_) | SirTy::Cap { .. } | SirTy::Fn { .. } => ct::I64,
        SirTy::Error | SirTy::Param(_) | SirTy::Module(_) => ct::I64,
        _ => return None,
    })
}

/// Total size, in bytes, of a SIR type (with the ADT catalog).
pub fn type_size(t: &SirTy, adts: &[AdtRef]) -> u32 {
    type_layout(t, adts).size
}

/// Alignment of a SIR type.
pub fn type_align(t: &SirTy, adts: &[AdtRef]) -> u32 {
    type_layout(t, adts).align
}

/// Round `size` up to the next pointer-sized boundary; used when sizing
/// stack slots for aggregates (cranelift wants pointer-aligned slots).
pub fn slot_size(size: u32) -> u32 {
    align_up(size.max(1), PTR_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdust_sir::sir::{AdtRef, AdtRefKind, FieldRef, VariantRef};
    use sdust_types::AdtId;

    fn make_struct(name: &str, field_tys: &[SirTy]) -> AdtRef {
        AdtRef {
            adt: AdtId(0),
            name: name.into(),
            kind: AdtRefKind::Struct,
            variants: vec![VariantRef {
                name: name.into(),
                fields: field_tys
                    .iter()
                    .map(|t| FieldRef {
                        name: None,
                        ty: t.clone(),
                    })
                    .collect(),
            }],
        }
    }

    #[test]
    fn struct_field_offsets_pack_naturally() {
        let s = make_struct("P", &[SirTy::Int(IntKind::I32), SirTy::Int(IntKind::I32)]);
        let (o0, _) = struct_field_offset(&s, 0, std::slice::from_ref(&s)).unwrap();
        let (o1, _) = struct_field_offset(&s, 1, std::slice::from_ref(&s)).unwrap();
        assert_eq!(o0, 0);
        assert_eq!(o1, 4);
    }

    #[test]
    fn enum_payload_after_tag() {
        let mut s = make_struct("E", &[SirTy::Int(IntKind::I32)]);
        // Two variants → enum.
        s.kind = AdtRefKind::Enum;
        s.variants.push(VariantRef {
            name: "V1".into(),
            fields: vec![FieldRef {
                name: None,
                ty: SirTy::Int(IntKind::I32),
            }],
        });
        let (o, _) = variant_field_offset(&s, 0, 0, &[s.clone()]).unwrap();
        // payload starts at align_up(4, 4) = 4
        assert_eq!(o, 4);
    }

    #[test]
    fn tuple_offsets_pack_naturally() {
        let elems = vec![SirTy::Bool, SirTy::Int(IntKind::I32)];
        let (o0, _) = tuple_offset(&elems, 0, &[]).unwrap();
        let (o1, _) = tuple_offset(&elems, 1, &[]).unwrap();
        assert_eq!(o0, 0);
        // bool(1) + pad(3) -> 4
        assert_eq!(o1, 4);
    }

    #[test]
    fn slot_size_rounds_up_to_pointer() {
        assert_eq!(slot_size(0), 8);
        assert_eq!(slot_size(7), 8);
        assert_eq!(slot_size(9), 16);
    }
}
