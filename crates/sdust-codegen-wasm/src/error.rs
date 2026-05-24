use thiserror::Error;

#[derive(Debug, Error)]
pub enum WasmError {
    #[error("wasm codegen: unsupported SIR shape: {0}")]
    Unsupported(String),
    #[error("wasm codegen: invalid module: {0}")]
    Invalid(String),
    #[error("wasm codegen: io error: {0}")]
    Io(String),
}

pub type CompileResult<T> = std::result::Result<T, WasmError>;
