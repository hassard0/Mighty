//! sdust-driver: compilation pipeline + manifest loader.
pub mod build;
pub mod manifest;
pub mod pipeline;
pub use build::{build_native, build_wasm, jit_run, BuildOptions, BuildOutcome, BuildTarget};
pub use manifest::Manifest;
pub use pipeline::{
    lower, lower_to_sir, parse_source, run_file, run_file_with_runtime, type_and_borrow_check,
    type_check, ParsedFile,
};
