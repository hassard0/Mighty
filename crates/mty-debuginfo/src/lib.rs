//! mty-debuginfo — debug-info generation for Mighty artifacts.
//!
//! Two surfaces:
//!
//! - [`dwarf`] — builds DWARF v4 sections (`.debug_info`, `.debug_line`,
//!   `.debug_str`, `.debug_abbrev`, `.debug_ranges`) for native object
//!   files. Used by `mty-codegen-cranelift` when `--debug` is set.
//! - [`sourcemap`] — emits source-map v3 JSON sidecars and Wasm `name`
//!   custom sections for `.wasm` artifacts. Used by
//!   `mty-codegen-wasm` when `--debug` is set.
//!
//! Both surfaces speak a small shared vocabulary
//! ([`FunctionDebugInfo`], [`SourcePos`]) so callers don't have to know
//! about gimli or source-map encoders directly.
//!
//! v0.2 scope (per planning doc):
//!
//! - DWARF: function-level `DW_TAG_subprogram` per fn + `DW_TAG_variable`
//!   per local (best-effort: name + type ref, no `.debug_loc` location
//!   lists yet); line program assembled from SIR `SourceSpan`s.
//! - Wasm: `name` custom section listing fn names + sidecar
//!   `<pkg>.wasm.map` source-map v3 mapping wasm byte offsets back to
//!   source positions.
//! - Deferred to v0.3: inlining info, generics info, full DWARF for the
//!   LLVM backend (build host lacks LLVM), `.debug_loc` per-local
//!   location lists.

pub mod dwarf;
pub mod sourcemap;

pub use dwarf::{DwarfBuilder, DwarfSection, DwarfSections, EncodedDwarf};
pub use sourcemap::{NameSection, SourceMap, SourceMapMapping};

/// Errors raised by the debug-info builders.
#[derive(Debug, thiserror::Error)]
pub enum DebugInfoError {
    #[error("dwarf write error: {0}")]
    DwarfWrite(String),
    #[error("source map error: {0}")]
    SourceMap(String),
}

pub type DebugInfoResult<T> = Result<T, DebugInfoError>;

/// A point in the original source. Byte offsets are absolute into the
/// source file. Line/column are 1-based per DWARF/source-map convention.
///
/// The codegen layer computes line/column from the SIR `SourceSpan`'s
/// byte offset using a precomputed line-index map (held outside this
/// crate so it can be shared with diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePos {
    pub byte_offset: u32,
    pub line: u32,
    pub column: u32,
}

impl SourcePos {
    pub fn new(byte_offset: u32, line: u32, column: u32) -> Self {
        Self {
            byte_offset,
            line,
            column,
        }
    }
}

/// Compute (1-based) line and column for a byte offset within `src`.
/// Used by callers to build [`SourcePos`] values from SIR spans.
pub fn line_col_for(src: &str, byte_offset: u32) -> (u32, u32) {
    let off = byte_offset as usize;
    let mut line: u32 = 1;
    let mut col: u32 = 1;
    for (i, ch) in src.char_indices() {
        if i >= off {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Per-local variable debug info (DWARF `DW_TAG_variable`).
#[derive(Debug, Clone)]
pub struct VarDebugInfo {
    pub name: String,
    /// Display form of the SIR type (best-effort string).
    pub type_name: String,
    /// Slot offset from frame base, when known.
    pub frame_offset: Option<i32>,
}

/// Per-function debug info supplied by the codegen layer. The DWARF
/// builder consumes this; the sourcemap builder consumes a subset
/// (name + start_pos + length + line_table).
#[derive(Debug, Clone)]
pub struct FunctionDebugInfo {
    pub name: String,
    pub mangled_name: Option<String>,
    /// Display form of the return type.
    pub return_type: String,
    /// Source position of the fn declaration (used as DWARF decl line).
    pub decl_pos: SourcePos,
    /// Code-address range in the object: (low_pc, high_pc).
    /// For object emission, low_pc is the function's section offset;
    /// the DWARF section is patched with relocations at link time. For
    /// v0.2 we don't emit relocations — DWARF refers to offsets within
    /// the section, which lldb/gdb can still walk for inspection.
    pub code_range: (u64, u64),
    /// Per-machine-instruction or per-SIR-stmt source position. The
    /// builder emits a DWARF line program covering every entry.
    pub line_table: Vec<(u64, SourcePos)>,
    /// Per-local variables.
    pub locals: Vec<VarDebugInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_first_line() {
        let src = "fn main() {}\n";
        let (l, c) = line_col_for(src, 3);
        assert_eq!(l, 1);
        assert_eq!(c, 4);
    }

    #[test]
    fn line_col_second_line() {
        let src = "abc\ndef";
        let (l, c) = line_col_for(src, 5);
        assert_eq!(l, 2);
        assert_eq!(c, 2);
    }

    #[test]
    fn line_col_zero_offset() {
        let src = "x";
        let (l, c) = line_col_for(src, 0);
        assert_eq!(l, 1);
        assert_eq!(c, 1);
    }
}
