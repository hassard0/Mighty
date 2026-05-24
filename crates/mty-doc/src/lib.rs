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

pub mod extract;
pub mod ir;
pub mod render;

pub use extract::build_doc_package;
pub use ir::{
    DocExample, DocItem, DocItemKind, DocModule, DocPackage, DocVisibility, ItemSignature,
};
