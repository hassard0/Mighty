//! sdust-diagnostics: diagnostic types and rendering.
pub mod codes;
pub mod diagnostic;
pub mod render;
pub use codes::DiagCode;
pub use diagnostic::{Diagnostic, Severity, Label};
