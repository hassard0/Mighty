//! DWARF integration for the cranelift native backend (v0.2).
//!
//! This module is intentionally thin: it builds a
//! [`mty_debuginfo::DwarfBuilder`] from a SIR program + the original
//! source text, then post-processes a cranelift `ObjectProduct` to
//! attach the encoded DWARF sections.
//!
//! What it covers in v0.2:
//!
//! - `DW_TAG_compile_unit` for the source file
//! - `DW_TAG_subprogram` per fn with name + return type + decl line
//! - `DW_TAG_variable` per non-temp local (best-effort)
//! - A coarse line program (one entry per fn entry-point; v0.2 doesn't
//!   yet plumb cranelift's `MachSrcLoc` map back into the line table)
//!
//! Deferred to v0.3:
//!
//! - Address::Symbol references so the linker patches low_pc/high_pc to
//!   real virtual addresses. We currently use Address::Constant(0) for
//!   each fn's low_pc and rely on the fn's offset in `.text` being 0
//!   for v0.2 inspectability — sufficient for `objdump --dwarf=info` to
//!   list the DIE structure, but not for live source-line stepping.
//! - Full line-table coverage (per machine-instr). Plumb cranelift
//!   `MachSrcLoc` events through `define_function` and call
//!   `DwarfBuilder::add_function` with the resulting per-instr table.
//! - `.debug_loc` per-local location lists keyed off cranelift slot
//!   offsets.

use crate::lower::FnSrcLocMap;
use mty_debuginfo::{
    line_col_for, DebugInfoError, Dwarf5Builder, DwarfBuilder, EncodedDwarf, FunctionDebugInfo,
    LineRow, LocalDebugInfo, SourcePos, VarDebugInfo,
};
use mty_ir::ir::{Function, IrFnId, IrTy, LocalSource, Program};
use std::collections::HashMap;

/// Per-build inputs used to assemble DWARF.
pub struct DwarfInputs<'a> {
    /// Original source text. Used to map SIR `SourceSpan` byte offsets
    /// to line/column pairs.
    pub source_text: &'a str,
    /// Display path for the source file (typically the path the user
    /// passed to `mty build`).
    pub source_path: &'a str,
    /// Working directory for `DW_AT_comp_dir` (typically `std::env::current_dir`).
    pub comp_dir: String,
}

/// Build a [`DwarfBuilder`] populated with one `DW_TAG_subprogram` per
/// function in `prog`. The returned builder is ready for `.finish()`;
/// the caller is responsible for setting the compile-unit's total
/// code size before finishing if it has that information.
pub fn build_dwarf_for(
    prog: &Program,
    inputs: &DwarfInputs<'_>,
) -> Result<DwarfBuilder, DebugInfoError> {
    let mut b = DwarfBuilder::new(inputs.source_path.to_string(), inputs.comp_dir.clone());
    b.init_compile_unit()?;
    // We assign each fn a 16-byte conservative placeholder for its
    // code range. v0.2 doesn't yet read the actual cranelift-emitted
    // size, but the placeholder keeps the DIE structure valid for
    // gimli round-trip and `objdump --dwarf=info` inspection.
    let mut low: u64 = 0;
    let placeholder_size: u64 = 16;
    let mut total: u64 = 0;
    for f in &prog.fns {
        let info = function_debug_info(f, inputs.source_text, low, placeholder_size);
        b.add_function(&info)?;
        low += placeholder_size;
        total += placeholder_size;
    }
    b.set_total_code_size(total);
    Ok(b)
}

/// Same shape as [`build_dwarf_for`] but produces DWARF v5 output via
/// [`Dwarf5Builder`]. v5 brings the indirect `.debug_line_str` string
/// table, str-offsets, loclists/rnglists, and a per-instruction line
/// program.
///
/// v0.21: when `srcloc_map` is non-empty, this path consumes
/// cranelift's per-instruction `MachSrcLoc` map for each fn and emits
/// dense, per-instruction line rows + `.debug_loclists` entries per
/// local. When `None`, falls back to the v0.20 conservative
/// placeholder shape.
pub fn build_dwarf5_for(
    prog: &Program,
    inputs: &DwarfInputs<'_>,
    srcloc_map: Option<&HashMap<IrFnId, FnSrcLocMap>>,
) -> Result<Dwarf5Builder, DebugInfoError> {
    let mut b = Dwarf5Builder::new(inputs.source_path.to_string(), inputs.comp_dir.clone());
    b.init_compile_unit()?;
    let mut low: u64 = 0;
    let placeholder_size: u64 = 16;
    let mut total: u64 = 0;
    for f in &prog.fns {
        let fn_dbg = srcloc_map.and_then(|m| m.get(&f.id));
        // Use the real compiled code size when we have it; otherwise
        // fall back to the v0.20 placeholder layout so callers
        // without per-fn debug info still produce valid DWARF.
        let size = match fn_dbg {
            Some(d) if d.code_size > 0 => d.code_size as u64,
            _ => placeholder_size,
        };
        let mut info = function_debug_info(f, inputs.source_text, low, size);
        if let Some(d) = fn_dbg {
            info.rich_line_table = rich_line_rows_for(d, inputs.source_text);
            info.rich_locals = rich_locals_for(f, d, low, low + size);
        }
        b.add_function(&info)?;
        low += size;
        total += size;
    }
    b.set_total_code_size(total);
    Ok(b)
}

/// Convert a [`FnSrcLocMap`] into a sequence of [`LineRow`]s usable by
/// [`Dwarf5Builder`]. Marks the first row of each statement as
/// `is_stmt = true` and sets `end_sequence = true` on the last row.
fn rich_line_rows_for(d: &FnSrcLocMap, src: &str) -> Vec<LineRow> {
    let mut rows = Vec::with_capacity(d.rows.len() + 1);
    if d.rows.is_empty() {
        return rows;
    }
    let mut last_src_idx: Option<u32> = None;
    let mut prev_off: u64 = 0;
    for (code_off, src_idx) in d.rows.iter() {
        let byte_off = d
            .stmt_byte_offsets
            .get(*src_idx as usize)
            .copied()
            .unwrap_or(0);
        let (line, col) = line_col_for(src, byte_off);
        let cur_off = *code_off as u64;
        // Defensive monotonicity: cranelift sorts MachSrcLoc by start,
        // but dedup may have left a stale `prev_off` if two
        // entries had the same start.
        if cur_off < prev_off {
            continue;
        }
        prev_off = cur_off;
        let is_stmt = last_src_idx != Some(*src_idx);
        last_src_idx = Some(*src_idx);
        rows.push(LineRow {
            address_offset: cur_off,
            line,
            column: col,
            is_stmt,
            // Don't mark end_sequence on intermediate rows; the
            // builder closes the sequence using `code_range` size by
            // default. We set end_sequence on the synthesized
            // terminator row below.
            end_sequence: false,
        });
    }
    // Synthesize a terminator row at code_size so the line program
    // closes cleanly. Mark `end_sequence = true` to flush.
    if let Some(last) = rows.last().copied() {
        if (d.code_size as u64) > last.address_offset {
            rows.push(LineRow {
                address_offset: d.code_size as u64,
                line: last.line,
                column: last.column,
                is_stmt: false,
                end_sequence: true,
            });
        } else {
            // No room for a synthetic terminator; mark the last row.
            if let Some(last_mut) = rows.last_mut() {
                last_mut.end_sequence = true;
            }
        }
    }
    rows
}

/// Build a [`LocalDebugInfo`] entry per user local in `f` using the
/// slot offsets cranelift recorded into `d.local_slot_offsets`.
fn rich_locals_for(f: &Function, d: &FnSrcLocMap, low: u64, high: u64) -> Vec<LocalDebugInfo> {
    let mut out = Vec::new();
    for (idx, decl) in f.locals.iter().enumerate() {
        if matches!(decl.source, LocalSource::Return | LocalSource::DropFlag) {
            continue;
        }
        // Default slot offset: -8 * (local_index + 1) so each local
        // gets a distinct synthetic offset even when cranelift hasn't
        // assigned a real stack slot. This is a conservative
        // placeholder — the loclist entry's `DW_OP_breg7` form keeps
        // the DIE structurally valid; v0.22 plumbs real slot
        // assignments via cranelift's `FrameLayout`.
        let slot = d
            .local_slot_offsets
            .get(&(idx as u32))
            .copied()
            .unwrap_or_else(|| -(8 * (idx as i32 + 1)));
        let name = if decl.name.is_empty() {
            format!("_{idx}")
        } else {
            decl.name.clone()
        };
        out.push(LocalDebugInfo {
            name,
            slot,
            address_range: (low, high),
            type_tag: display_ty(&decl.ty),
        });
    }
    out
}

/// Returns true when the build should emit DWARF v5 rather than v4.
///
/// Toggled by the `MTY_DWARF5=1` env var. Default is v4 for
/// back-compat with downstream DWARF parsers in CI and external tools.
pub fn dwarf5_enabled() -> bool {
    std::env::var("MTY_DWARF5")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes"))
        .unwrap_or(false)
}

/// Dispatch helper: build + finish either a v4 or v5 DWARF blob per the
/// env-var toggle, returning the encoded section bytes ready to attach
/// to an object file.
///
/// v0.21: the v5 path now consumes the per-fn `MachSrcLoc` map
/// captured by `LowerCtx::define_fn`. The v4 path stays unchanged
/// (conservative placeholder line table) so downstream parsers that
/// haven't moved to v5 keep seeing the v0.20 baseline.
pub fn build_dwarf_dispatch(
    prog: &Program,
    inputs: &DwarfInputs<'_>,
    srcloc_map: Option<&HashMap<IrFnId, FnSrcLocMap>>,
) -> Result<EncodedDwarf, DebugInfoError> {
    if dwarf5_enabled() {
        build_dwarf5_for(prog, inputs, srcloc_map)?.finish()
    } else {
        build_dwarf_for(prog, inputs)?.finish()
    }
}

/// Build a [`FunctionDebugInfo`] for a single SIR fn.
pub fn function_debug_info(f: &Function, src: &str, low_pc: u64, size: u64) -> FunctionDebugInfo {
    let (line, col) = line_col_for(src, f.span.start);
    let decl_pos = SourcePos::new(f.span.start, line, col);
    let mut line_table = Vec::new();
    line_table.push((0, decl_pos));
    // Best-effort: one entry per block, pointing at the fn start (we
    // don't yet have per-stmt source spans plumbed all the way to
    // SIR statements; that's a v0.3 follow-up). The line program is
    // still valid and consumers will see at least the fn entry row.
    if size > 1 {
        line_table.push((size - 1, decl_pos));
    }
    let mut locals = Vec::new();
    for (idx, decl) in f.locals.iter().enumerate() {
        // Skip the return-slot (`_0`) and synthetic drop flags; emit
        // params + user lets + temps with names.
        if matches!(decl.source, LocalSource::Return | LocalSource::DropFlag) {
            continue;
        }
        let name = if decl.name.is_empty() {
            format!("_{idx}")
        } else {
            decl.name.clone()
        };
        locals.push(VarDebugInfo {
            name,
            type_name: display_ty(&decl.ty),
            // v0.2: no real frame offset (cranelift's slot offsets
            // aren't exposed by define_function yet). v0.3 wires this.
            frame_offset: None,
        });
    }
    FunctionDebugInfo {
        name: f.name.clone(),
        mangled_name: None,
        return_type: display_ty(&f.ret_ty),
        decl_pos,
        code_range: (low_pc, low_pc + size),
        line_table,
        locals,
        rich_line_table: Vec::new(),
        rich_locals: Vec::new(),
    }
}

/// Best-effort display form for a SIR type. Mirrors the type names the
/// DWARF base-type table understands.
pub fn display_ty(t: &IrTy) -> String {
    match t {
        IrTy::Bool => "bool".into(),
        IrTy::Char => "char".into(),
        IrTy::Str => "str".into(),
        IrTy::String => "String".into(),
        IrTy::Bytes => "Bytes".into(),
        IrTy::Unit => "()".into(),
        IrTy::Never => "!".into(),
        IrTy::Duration => "duration".into(),
        IrTy::Size => "size".into(),
        IrTy::Int(k) => format!("{k:?}").to_lowercase(),
        IrTy::Float(k) => format!("{k:?}").to_lowercase(),
        IrTy::Tuple(elems) => {
            let parts: Vec<_> = elems.iter().map(display_ty).collect();
            format!("({})", parts.join(", "))
        }
        IrTy::Array { elem, len } => match len {
            Some(n) => format!("[{}; {}]", display_ty(elem), n),
            None => format!("[{}]", display_ty(elem)),
        },
        IrTy::Ref { mutable, inner } => {
            if *mutable {
                format!("&mut {}", display_ty(inner))
            } else {
                format!("&{}", display_ty(inner))
            }
        }
        IrTy::Fn { params, ret } => {
            let parts: Vec<_> = params.iter().map(display_ty).collect();
            format!("fn({}) -> {}", parts.join(", "), display_ty(ret))
        }
        IrTy::Adt(_, _) => "adt".into(),
        IrTy::Cap { family, .. } => format!("Cap<{family:?}>"),
        IrTy::Dyn(name) => format!("dyn {name}"),
        IrTy::RawPtr(inner) => format!("*const {}", display_ty(inner)),
        IrTy::Module(name) => format!("module<{name}>"),
        IrTy::Param(name) => name.clone(),
        IrTy::Error => "error".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mty_hir::SourceSpan;
    use mty_ir::ir::{Block, BlockId, Const, Function, IrFnId, LocalDecl, Operand, Term};
    use mty_types::IntKind;

    fn dummy_main() -> Function {
        Function {
            id: IrFnId(0),
            name: "main".into(),
            params: vec![],
            locals: vec![
                LocalDecl {
                    name: "_0".into(),
                    ty: IrTy::Int(IntKind::I32),
                    mutable: false,
                    source: LocalSource::Return,
                },
                LocalDecl {
                    name: "n".into(),
                    ty: IrTy::Int(IntKind::I32),
                    mutable: false,
                    source: LocalSource::UserLet,
                },
            ],
            blocks: vec![Block {
                id: BlockId(0),
                stmts: vec![],
                terminator: Term::Return(Operand::Const(Const::Unit)),
            }],
            entry: BlockId(0),
            ret_ty: IrTy::Int(IntKind::I32),
            effects: vec![],
            hir_fn: None,
            span: SourceSpan { start: 0, end: 12 },
        }
    }

    #[test]
    fn function_debug_info_includes_user_locals() {
        let f = dummy_main();
        let info = function_debug_info(&f, "fn main() {}\n", 0, 16);
        assert_eq!(info.name, "main");
        assert_eq!(info.return_type, "i32");
        assert_eq!(info.locals.len(), 1);
        assert_eq!(info.locals[0].name, "n");
        assert_eq!(info.locals[0].type_name, "i32");
        assert_eq!(info.code_range, (0, 16));
    }

    #[test]
    fn build_dwarf_emits_sections() {
        let mut prog = Program::default();
        prog.fns.push(dummy_main());
        let inputs = DwarfInputs {
            source_text: "fn main() {}\n",
            source_path: "x.mty",
            comp_dir: "/tmp".into(),
        };
        let b = build_dwarf_for(&prog, &inputs).unwrap();
        let enc = b.finish().unwrap();
        assert!(enc.sections.iter().any(|s| s.name == ".debug_info"));
    }

    #[test]
    fn build_dwarf5_emits_indirect_str_section() {
        let mut prog = Program::default();
        prog.fns.push(dummy_main());
        let inputs = DwarfInputs {
            source_text: "fn main() {}\n",
            source_path: "x.mty",
            comp_dir: "/tmp".into(),
        };
        let b = build_dwarf5_for(&prog, &inputs, None).unwrap();
        let enc = b.finish().unwrap();
        // v5 must produce .debug_line_str when comp_dir/comp_file go
        // through LineString::LineStringRef.
        assert!(enc.sections.iter().any(|s| s.name == ".debug_line_str"));
        assert!(enc.sections.iter().any(|s| s.name == ".debug_info"));
    }

    #[test]
    fn display_ty_examples() {
        assert_eq!(display_ty(&IrTy::Bool), "bool");
        assert_eq!(display_ty(&IrTy::Int(IntKind::I32)), "i32");
        assert_eq!(
            display_ty(&IrTy::Tuple(vec![IrTy::Bool, IrTy::Int(IntKind::I32)])),
            "(bool, i32)"
        );
    }
}
