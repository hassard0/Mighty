//! ADT and primitive layout for the slice-8 codegen.
//!
//! Slice-8 uses simple sequential layout with natural alignment. No
//! niche optimization, no field reordering. The size/alignment of
//! aggregate types follows C rules.

use mty_ir::ir::{AdtRef, IrTy};
use mty_types::{FloatKind, IntKind};

#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub size: u32,
    pub align: u32,
}

impl Layout {
    pub const ZST: Layout = Layout { size: 0, align: 1 };
    pub fn scalar(bytes: u32) -> Self {
        Self {
            size: bytes,
            align: bytes.max(1),
        }
    }
}

pub const PTR_BYTES: u32 = 8; // slice-8 ships 64-bit only

/// Compute layout for a primitive SIR type. Aggregate types require
/// the AdtCatalog; see [`layout_with_adts`].
pub fn primitive_layout(t: &IrTy) -> Option<Layout> {
    Some(match t {
        IrTy::Bool => Layout::scalar(1),
        IrTy::Char => Layout::scalar(4),
        IrTy::Unit | IrTy::Never => Layout::ZST,
        IrTy::Int(k) => match k {
            IntKind::I8 | IntKind::U8 => Layout::scalar(1),
            IntKind::I16 | IntKind::U16 => Layout::scalar(2),
            IntKind::I32 | IntKind::U32 | IntKind::IntInfer => Layout::scalar(4),
            IntKind::I64 | IntKind::U64 => Layout::scalar(8),
            IntKind::ISize | IntKind::USize => Layout::scalar(PTR_BYTES),
            IntKind::I128 | IntKind::U128 => Layout::scalar(16),
        },
        IrTy::Float(k) => match k {
            FloatKind::F32 => Layout::scalar(4),
            FloatKind::F64 | FloatKind::FloatInfer => Layout::scalar(8),
        },
        IrTy::Duration | IrTy::Size => Layout::scalar(8),
        // Str / String / Bytes share (ptr, len) shape.
        IrTy::Str | IrTy::String | IrTy::Bytes => Layout {
            size: PTR_BYTES * 2,
            align: PTR_BYTES,
        },
        IrTy::Ref { .. } | IrTy::RawPtr(_) | IrTy::Cap { .. } => Layout::scalar(PTR_BYTES),
        IrTy::Dyn(_) => Layout {
            size: PTR_BYTES * 2,
            align: PTR_BYTES,
        },
        IrTy::Fn { .. } => Layout::scalar(PTR_BYTES),
        IrTy::Module(_) | IrTy::Param(_) | IrTy::Error => Layout::ZST,
        // Aggregates need the catalog.
        IrTy::Tuple(_) | IrTy::Array { .. } | IrTy::Adt(_, _) => return None,
    })
}

/// Compute a layout including aggregate (tuple/array/ADT) types.
pub fn layout_with_adts(t: &IrTy, adts: &[AdtRef]) -> Layout {
    if let Some(l) = primitive_layout(t) {
        return l;
    }
    match t {
        IrTy::Tuple(elems) => layout_struct(elems.iter().map(|e| layout_with_adts(e, adts))),
        IrTy::Array { elem, len } => {
            let inner = layout_with_adts(elem, adts);
            let n = len.unwrap_or(0);
            Layout {
                size: inner.size * (n as u32),
                align: inner.align,
            }
        }
        IrTy::Adt(id, _args) => match adts.iter().find(|a| a.adt == *id) {
            Some(adt) => {
                if adt.variants.is_empty() {
                    // Opaque ADT with no variants — treat as pointer-sized.
                    Layout::scalar(PTR_BYTES)
                } else if adt.variants.len() == 1 {
                    let v = &adt.variants[0];
                    layout_struct(v.fields.iter().map(|f| layout_with_adts(&f.ty, adts)))
                } else {
                    // Enum: tag (u32) + max variant payload, naturally aligned.
                    let tag = Layout::scalar(4);
                    let payload = adt
                        .variants
                        .iter()
                        .map(|v| {
                            layout_struct(v.fields.iter().map(|f| layout_with_adts(&f.ty, adts)))
                        })
                        .fold(Layout::ZST, |acc, l| Layout {
                            size: acc.size.max(l.size),
                            align: acc.align.max(l.align),
                        });
                    layout_struct([tag, payload].into_iter())
                }
            }
            None => Layout::scalar(PTR_BYTES),
        },
        _ => Layout::scalar(PTR_BYTES),
    }
}

/// Lay out a struct with natural alignment; size is rounded up to
/// the struct's alignment.
fn layout_struct(fields: impl Iterator<Item = Layout>) -> Layout {
    let mut size: u32 = 0;
    let mut align: u32 = 1;
    for f in fields {
        align = align.max(f.align);
        size = align_up(size, f.align);
        size += f.size;
    }
    Layout {
        size: align_up(size, align),
        align,
    }
}

pub fn align_up(v: u32, a: u32) -> u32 {
    debug_assert!(a.is_power_of_two() && a > 0);
    (v + a - 1) & !(a - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_have_expected_sizes() {
        assert_eq!(primitive_layout(&IrTy::Bool).unwrap().size, 1);
        assert_eq!(primitive_layout(&IrTy::Int(IntKind::I64)).unwrap().size, 8);
        assert_eq!(
            primitive_layout(&IrTy::Float(FloatKind::F32)).unwrap().size,
            4
        );
        assert_eq!(primitive_layout(&IrTy::Str).unwrap().size, PTR_BYTES * 2);
        assert_eq!(primitive_layout(&IrTy::Unit).unwrap().size, 0);
    }

    #[test]
    fn align_up_rounds_correctly() {
        assert_eq!(align_up(7, 8), 8);
        assert_eq!(align_up(8, 8), 8);
        assert_eq!(align_up(9, 8), 16);
        assert_eq!(align_up(0, 4), 0);
    }

    #[test]
    fn struct_layout_naturally_aligned() {
        let l =
            layout_struct([Layout::scalar(1), Layout::scalar(4), Layout::scalar(2)].into_iter());
        // u8 + pad3 + u32 + u16 + pad2 = 12 bytes, align 4
        assert_eq!(l.size, 12);
        assert_eq!(l.align, 4);
    }
}
