//! Debug-info sidecar generation for the wasm backend (v0.2).
//!
//! Two outputs:
//!
//! 1. **`name` custom section** — wasm-spec convention listing function
//!    names so DevTools and `wasm-objdump` show readable identifiers
//!    instead of `wasm-function[N]`. The bytes are appended to a wasm
//!    module via [`append_name_section`].
//!
//! 2. **Source-map v3 JSON sidecar** — written to `<out>.wasm.map`,
//!    discoverable via a `sourceMappingURL` custom section appended
//!    to the wasm module. DevTools and Chrome load the sidecar
//!    automatically. The wasm-tool-conventions debugging spec
//!    documents the format; we follow it as of v0.2 of that spec.
//!
//! This module is wire-compatible with the bare-core-wasm path; the
//! Component Model wrapper preserves custom sections it doesn't
//! recognize, so the same machinery works for both.
//!
//! v0.2 scope:
//! - Function names only in the `name` section (subsection id 1).
//! - One source-map mapping per fn-entry (slice-8 doesn't track
//!   per-instr source spans, so a coarser mapping is the best we can
//!   honestly emit).
//!
//! Deferred (v0.3):
//! - Local names (`name` subsection id 2).
//! - Per-stmt mappings once SIR statements carry SourceSpan.

use crate::error::{CompileResult, WasmError};
use mty_debuginfo::sourcemap::{
    source_mapping_url_section, NameSection, SourceMap, SourceMapMapping, WasmFnName,
};
use mty_debuginfo::{line_col_for, SourcePos};
use mty_ir::ir::Program;
use std::path::{Path, PathBuf};

/// Build a `NameSection` listing every user-defined fn in `prog`.
/// The import-count argument is the number of imported functions
/// (which take fn-indices 0..N before user fns).
pub fn build_name_section(prog: &Program, import_count: u32) -> NameSection {
    let mut ns = NameSection::new();
    for (i, f) in prog.fns.iter().enumerate() {
        ns.functions.push(WasmFnName {
            index: import_count + i as u32,
            name: f.name.clone(),
        });
    }
    ns
}

/// Build a coarse source-map for `prog`. v0.2 emits one mapping per fn
/// — the fn's entry-point byte offset 0 → the fn's SIR span.
///
/// `source_path` is the relative path the source-map's `sources` array
/// will record. `source_text` is the literal source (used to compute
/// line/column from byte offsets, and stored as `sourcesContent[0]` so
/// debuggers can render source even if the .mty file isn't fetched).
pub fn build_source_map(
    prog: &Program,
    source_path: &str,
    source_text: &str,
    output_wasm: &str,
) -> SourceMap {
    let mut sm = SourceMap::new();
    sm.file = Some(output_wasm.to_string());
    let src_idx = sm.add_source(source_path, Some(source_text.to_string()));
    for f in &prog.fns {
        let (line, col) = line_col_for(source_text, f.span.start);
        sm.add_mapping(SourceMapMapping::from_pos(
            // v0.2 uses fn-index as a stand-in for byte offset; once
            // we plumb code-section offsets through emit, this becomes
            // the real generated_offset.
            f.id.0,
            src_idx,
            SourcePos::new(f.span.start, line, col),
        ));
    }
    sm
}

/// Append the `name` custom section bytes and a `sourceMappingURL`
/// custom section pointing at `<sidecar_filename>` to the wasm bytes
/// in place. Returns the modified bytes.
///
/// The wasm binary format permits custom sections at the end of the
/// module — runtimes ignore unknown ones and validators accept any
/// number of them. The Component-Model encoder also preserves them.
pub fn append_debug_sections(
    mut wasm: Vec<u8>,
    name_section: &NameSection,
    sidecar_url: &str,
) -> Vec<u8> {
    let name_bytes = name_section.encode_full_section();
    wasm.extend_from_slice(&name_bytes);
    let url_bytes = source_mapping_url_section(sidecar_url);
    wasm.extend_from_slice(&url_bytes);
    wasm
}

/// Write the source-map sidecar to disk at `<output_wasm>.map`.
/// Returns the sidecar path.
pub fn write_sourcemap_sidecar(output_wasm: &Path, sm: &SourceMap) -> CompileResult<PathBuf> {
    let sidecar = sourcemap_sidecar_path(output_wasm);
    let bytes = sm
        .to_json()
        .map_err(|e| WasmError::Io(format!("sourcemap encode: {e}")))?;
    std::fs::write(&sidecar, &bytes)
        .map_err(|e| WasmError::Io(format!("write {}: {}", sidecar.display(), e)))?;
    Ok(sidecar)
}

/// Compute the sidecar path: `<output>.wasm.map`.
pub fn sourcemap_sidecar_path(output_wasm: &Path) -> PathBuf {
    let mut s = output_wasm.as_os_str().to_owned();
    s.push(".map");
    PathBuf::from(s)
}

/// Return just the filename portion of a sidecar path — what the
/// `sourceMappingURL` custom section should reference (DevTools
/// resolves the URL relative to the wasm fetch URL).
pub fn sidecar_relative_filename(sidecar: &Path) -> String {
    sidecar
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| sidecar.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mty_hir::SourceSpan;
    use mty_ir::ir::{
        Block, BlockId, Const, Function, IrFnId, IrTy, LocalDecl, LocalSource, Operand, Term,
    };

    fn empty_main() -> Program {
        let mut p = Program::default();
        p.fns.push(Function {
            id: IrFnId(0),
            name: "main".into(),
            params: vec![],
            locals: vec![LocalDecl {
                name: "_0".into(),
                ty: IrTy::Unit,
                mutable: false,
                source: LocalSource::Return,
            }],
            blocks: vec![Block {
                id: BlockId(0),
                stmts: vec![],
                terminator: Term::Return(Operand::Const(Const::Unit)),
            }],
            entry: BlockId(0),
            ret_ty: IrTy::Unit,
            effects: vec![],
            hir_fn: None,
            span: SourceSpan { start: 3, end: 12 },
        });
        p
    }

    #[test]
    fn name_section_includes_user_fns() {
        let p = empty_main();
        let ns = build_name_section(&p, 1); // 1 import
        assert_eq!(ns.functions.len(), 1);
        assert_eq!(ns.functions[0].name, "main");
        assert_eq!(ns.functions[0].index, 1);
    }

    #[test]
    fn source_map_includes_fn_entry() {
        let p = empty_main();
        let sm = build_source_map(&p, "hello.mty", "// hi\nfn main() {}\n", "hello.wasm");
        assert_eq!(sm.sources, vec!["hello.mty"]);
        assert_eq!(sm.file.as_deref(), Some("hello.wasm"));
        assert_eq!(sm.mappings.len(), 1);
    }

    #[test]
    fn sidecar_path_appends_map_suffix() {
        let p = Path::new("target/hello.wasm");
        let s = sourcemap_sidecar_path(p);
        assert_eq!(s.file_name().unwrap().to_string_lossy(), "hello.wasm.map");
    }

    #[test]
    fn sidecar_relative_filename_strips_dirs() {
        let p = sourcemap_sidecar_path(Path::new("a/b/c/hello.wasm"));
        assert_eq!(sidecar_relative_filename(&p), "hello.wasm.map");
    }

    #[test]
    fn append_debug_sections_preserves_prior_bytes() {
        let original = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]; // wasm magic + version
        let ns = build_name_section(&empty_main(), 0);
        let modified = append_debug_sections(original.clone(), &ns, "hello.wasm.map");
        assert_eq!(&modified[..original.len()], &original[..]);
        assert!(modified.len() > original.len());
    }
}
