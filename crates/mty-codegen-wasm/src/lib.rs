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
pub mod preview2;
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
pub use preview2::{
    build_direct_p2_probe_module, compile_program_to_bytes_p2, compile_program_to_file_p2,
    emit_wit_p2, AdapterKind, P2DirectImport, Preview2Options, UserWit, VENDORED_WASI_P2_WIT,
    WASI_P1_ADAPTER_COMMAND, WASI_P1_ADAPTER_PROXY, WASI_P1_ADAPTER_REACTOR,
    WASI_P1_ADAPTER_VERSION, WASI_P2_VERSION,
};
pub use target::WasmTarget;
pub use wit::{emit_wit, WitDocument};
