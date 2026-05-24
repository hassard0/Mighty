//! sdust-driver: compilation pipeline + manifest loader.
pub mod manifest;
pub mod pipeline;
pub use manifest::Manifest;
pub use pipeline::{lower, parse_source, ParsedFile};
