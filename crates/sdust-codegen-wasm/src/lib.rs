//! sdust-codegen-wasm: SIR → Wasm core module (slice 8).
//!
//! Emits a core Wasm module per package using `wasm-encoder`. Slice-8
//! covers the same conservative SIR subset as the Cranelift backend:
//!
//! - integer / bool / float arithmetic & comparisons
//! - locals (Wasm `local.get`/`local.set`)
//! - `log("...")` and `print("...")` via host imports
//! - `if` / `goto` (via Wasm block/loop/br_if) / `return`
//! - immediate string constants via the data section
//!
//! Anything else raises [`WasmError::Unsupported`], which the driver
//! reports back to the user (we don't fall back to interpreter for
//! the wasm target — there's no interpreter on the wasm side).

pub mod artifact;
pub mod emit;
pub mod error;
pub mod target;

pub use artifact::WasmArtifact;
pub use emit::{compile_program, compile_program_to_bytes, compile_program_to_file};
pub use error::{CompileResult, WasmError};
pub use target::WasmTarget;
