//! sdust-driver: compilation pipeline + manifest loader.
pub mod manifest;
pub mod pipeline;
pub use manifest::Manifest;
pub use pipeline::{
    lower, lower_to_sir, parse_source, run_file, type_and_borrow_check, type_check, ParsedFile,
};
