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

use mty_debuginfo::{
    line_col_for, DebugInfoError, DwarfBuilder, FunctionDebugInfo, SourcePos, VarDebugInfo,
};
use mty_ir::ir::{Function, LocalSource, Program, IrTy};

/// Per-build inputs used to assemble DWARF.
pub struct DwarfInputs<'a> {
    /// Original source text. Used to map SIR `SourceSpan` byte offsets
    /// to line/column pairs.
    pub source_text: &'a str,
    /// Display path for the source file (typically the path the user
    /// passed to `sdust build`).
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
    use mty_ir::ir::{Block, BlockId, Const, Function, LocalDecl, Operand, IrFnId, Term};
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
            source_path: "x.sd",
            comp_dir: "/tmp".into(),
        };
        let b = build_dwarf_for(&prog, &inputs).unwrap();
        let enc = b.finish().unwrap();
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
