//! mty-codegen-wasm: SIR → Wasm Component Model (v0.2).
//!
//! Emits a Component-Model `.wasm` per package. The pipeline is:
//!
//! 1. Lower SIR into a core Wasm module via [`emit`].
//! 2. Generate a WIT contract via [`wit`].
//! 3. Embed the WIT into the core module and wrap it as a
//!    Component Model component via [`component`].
//!
//! Closes amendment A47: full Component Model output is no longer
//! deferred. The bare core module is still produced (and writable
//! via [`compile_program_to_file_with_options`] with
//! `BuildOptions::core_only`) for users on runtimes that don't yet
//! support the Component Model.
//!
//! ### Core SIR coverage
//!
//! The core-module lowerer is intentionally narrow (mirrors slice 8
//! and is co-evolved with the Cranelift backend). On the first SIR
//! shape it can't handle, the function body is reset to a single
//! `unreachable` instruction so the resulting module still
//! validates. See `docs/internals/codegen-wasm.md` for the
//! current coverage matrix.

pub mod artifact;
pub mod component;
pub mod emit;
pub mod error;
pub mod sourcemap;
pub mod target;
pub mod wit;

pub use artifact::WasmArtifact;
pub use component::{is_component, wrap_as_component};
pub use emit::{
    compile_program, compile_program_to_bytes, compile_program_to_file,
    compile_program_to_file_with_options, BuildOptions,
};
pub use error::{CompileResult, WasmError};
pub use target::WasmTarget;
pub use wit::{emit_wit, WitDocument};
