//! Wasm artifact descriptor.
//!
//! Holds the final on-disk bytes (which may be either a core module
//! or a Component Model component) plus, when applicable, the
//! sidecar core-module bytes that `--no-component` requested.

use crate::target::WasmTarget;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmFormat {
    /// Core Wasm module (`\0asm\x01\x00\x00\x00`).
    CoreModule,
    /// Component Model component (`\0asm\x0d\x00\x01\x00` or
    /// equivalent layer-1 preamble).
    Component,
}

#[derive(Debug, Clone)]
pub struct WasmArtifact {
    pub bytes: Vec<u8>,
    pub path: Option<PathBuf>,
    pub target: WasmTarget,
    pub format: WasmFormat,
    /// When `--no-component` is set, the bare core module is also
    /// written alongside as `<name>.core.wasm` and the path is
    /// remembered here. `bytes`/`path` still describe the *primary*
    /// artifact, which is the core module in that mode.
    pub sidecar_core_path: Option<PathBuf>,
    /// The generated WIT (always populated, even in core-only mode,
    /// so downstream tools can read it).
    pub wit_text: Option<String>,
}

impl WasmArtifact {
    pub fn validate(&self) -> Result<(), wasmparser::BinaryReaderError> {
        let mut validator =
            wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
        validator.validate_all(&self.bytes).map(|_| ())
    }
}
