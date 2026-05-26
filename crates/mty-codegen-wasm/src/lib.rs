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
pub mod cabi_realloc;
pub mod component;
pub mod emit;
pub mod error;
pub mod preview2;
pub mod sourcemap;
pub mod target;
pub mod web_lower;
pub mod wit;

pub use artifact::WasmArtifact;
pub use cabi_realloc::{
    build_cabi_realloc_body, emit_size_class, CABI_REALLOC_HEAP_BASE, CABI_REALLOC_LARGE_THRESHOLD,
    CABI_REALLOC_NUM_CLASSES, CABI_REALLOC_STATE_BASE,
};
pub use component::{is_component, wrap_as_component};
pub use emit::{
    compile_program, compile_program_to_bytes, compile_program_to_bytes_with_preview,
    compile_program_to_file, compile_program_to_file_with_options, BuildOptions, EmitWasiPreview,
};
pub use error::{CompileResult, WasmError};
pub use preview2::{
    build_direct_p2_probe_module, canonical_abi_descriptor_signature,
    canonical_abi_outgoing_request_signature, compile_program_to_bytes_p2,
    compile_program_to_file_p2, emit_log_call_sequence, emit_resource_borrow_passthrough,
    emit_resource_drop_call, emit_wit_p2, AdapterEmbed, AdapterKind, P2DirectImport,
    Preview2Options, UserWit, CANONICAL_ABI_DESCRIPTOR_STAT_SIZE, VENDORED_WASI_P2_WIT,
    WASI_P2_VERSION,
};
pub use target::WasmTarget;
pub use web_lower::{
    canvas_signature, ensure_canvas_import, is_web_callback_export, CanvasImports, CANVAS_MODULE,
};
pub use wit::{emit_wit, WitDocument};
