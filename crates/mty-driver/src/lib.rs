//! mty-driver: compilation pipeline + manifest loader.
//
// v0.35 T1 — the codegen/runtime-dependent surface (`build`,
// `pipeline::run_file*`) is gated behind the `host-toolchain` feature
// so the lightweight parse/lower/typeck path can be compiled to
// `wasm32-unknown-unknown` for the in-browser playground.
#[cfg(feature = "host-toolchain")]
pub mod build;
pub mod link_flavor;
pub mod manifest;
pub mod pipeline;
#[cfg(feature = "host-toolchain")]
pub use build::{build_native, build_wasm, jit_run, BuildOptions, BuildOutcome, BuildTarget};
pub use manifest::Manifest;
pub use pipeline::{
    check_use_resolution, discover_package_sources, find_manifest_root, lower, lower_files,
    lower_files_with_ownership, lower_to_sir, parse_source, type_and_borrow_check,
    type_and_borrow_check_with_opts, type_check, ParsedFile,
};
#[cfg(feature = "host-toolchain")]
pub use pipeline::{run_file, run_file_with_runtime};
