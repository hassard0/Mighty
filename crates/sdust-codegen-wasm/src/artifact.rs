//! Wasm artifact descriptor.

use crate::target::WasmTarget;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct WasmArtifact {
    pub bytes: Vec<u8>,
    pub path: Option<PathBuf>,
    pub target: WasmTarget,
}

impl WasmArtifact {
    pub fn validate(&self) -> Result<(), wasmparser::BinaryReaderError> {
        let mut validator = wasmparser::Validator::new();
        validator.validate_all(&self.bytes).map(|_| ())
    }
}
