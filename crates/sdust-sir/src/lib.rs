//! sdust-sir: Stardust mid-level IR + interpreter (spec §24.4, §31.4).
//!
//! Slice 6 deliverable. Consumes typed + borrow-checked HIR and emits a
//! basic-block IR (`sir::Program`) plus a tree-walking interpreter that
//! executes the safe subset of Stardust.
//!
//! Public surface:
//!
//! - [`lower::lower_package`] — HIR → SIR
//! - [`interp::run`] — execute a `Program` with a [`interp::Host`]
//! - [`dump::dump_program`] — render SIR as text (used by `sdust dump --sir`)
//!
//! Slice-6 limitations are documented in
//! `docs/superpowers/specs/2026-05-24-slice6-sir-interpreter-design.md`.

pub mod dump;
pub mod interp;
pub mod lower;
pub mod sir;

pub use dump::dump_program;
pub use lower::lower_package;
pub use sir::*;
