//! mty-ir: Mighty mid-level IR + interpreter (spec §24.4, §31.4).
//!
//! Slice 6 deliverable. Consumes typed + borrow-checked HIR and emits a
//! basic-block IR (`ir::Program`) plus a tree-walking interpreter that
//! executes the safe subset of Mighty.
//!
//! Public surface:
//!
//! - [`lower::lower_package`] — HIR → IR
//! - [`interp::run`] — execute a `Program` with a [`interp::Host`]
//! - [`dump::dump_program`] — render IR as text (used by `mty dump --ir`)
//!
//! Slice-6 limitations are documented in
//! `docs/superpowers/specs/2026-05-24-slice6-sir-interpreter-design.md`.

pub mod dump;
pub mod interp;
pub mod ir;
pub mod lower;

pub use dump::dump_program;
pub use ir::*;
pub use lower::lower_package;
