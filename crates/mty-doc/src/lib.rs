//! mty-doc: Mighty documentation generator.
//!
//! Walks a parsed package, harvests `///` doc comments off each
//! top-level item (and `//!` comments off the file head for the
//! package-level synopsis), parses the bodies as CommonMark, and
//! renders them as plain text (Go-style), Markdown, or HTML.
//!
//! Entry points:
//!
//! - [`build_doc_package`] — parse a `.mty` source and produce a
//!   [`DocPackage`] IR.
//! - [`render::text`] — Go-style stdout output (`mty doc`).
//! - [`render::item_text`] — single-item Go-style output
//!   (`mty doc Item`).
//! - [`render::markdown`] — full markdown tree (one file per module).
//! - [`render::html`] — full HTML site (per-module pages + index +
//!   embedded CSS + search index).
//!
//! See `docs/internals/doc-generator.md` for design notes.

pub mod examples;
pub mod extract;
pub mod ir;
pub mod render;
pub mod stdlib_walker;
pub mod surface_audit;

pub use examples::{
    examples_count, examples_to_json, infer_see_also, lookup as lookup_stdlib_example,
    lookup_method as lookup_stdlib_method, persist_examples_index, render_hover_markdown,
    stdlib_examples_hash, symbols as stdlib_example_symbols, StdlibExample, STDLIB_EXAMPLES,
};
pub use extract::build_doc_package;
pub use ir::{
    DocExample, DocItem, DocItemKind, DocModule, DocPackage, DocVisibility, ItemSignature,
};
pub use stdlib_walker::{
    build_extracted_catalog, diff_catalogs, lookup_extracted, parse_docstub, render_drift_report,
    Drift, DriftKind, ExtractedExample, StdlibExampleRef,
};
pub use surface_audit::{audit_catalog, render_audit_report, RealSurface, UnresolvedEntry};
