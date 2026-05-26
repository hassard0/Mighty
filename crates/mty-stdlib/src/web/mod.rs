//! `std.web` — Mighty-side bindings for the `mty:web/*` WIT interfaces.
//!
//! v0.23 (Track A) ships the canvas + input surfaces:
//!
//! - [`canvas`] — `Canvas` resource wrapping the host's 2D drawing
//!   context. Maps 1:1 onto `mty:web/canvas@0.1`.
//! - [`input`]  — `Input` resource + `Key` enum. Maps onto
//!   `mty:web/input@0.1` with a host-string → `Key` decoder so guest
//!   agents don't have to know about the DOM string vocabulary.
//!
//! Both resources are *opaque handles* on the Mighty side: their
//! methods lower to direct WIT imports in `mty-codegen-wasm` when
//! the program is compiled for `wasm32-web`. The native runtime path
//! (used by `mty run` and the JIT) stubs the methods out — the
//! native host has no canvas / window, so calls become no-ops.
//!
//! The interfaces here are the single source of truth for the
//! Mighty-side surface; the WIT shape lives in
//! `crates/mty-codegen-wasm/wit/mty-web/{canvas,input,world}.wit`,
//! and Track D's host shim binds the imports to the browser DOM.

pub mod canvas;
pub mod input;

pub use canvas::Canvas;
pub use input::{Input, Key};

/// Canonical WIT interface name for `canvas`. The codegen-wasm crate
/// pattern-matches on this string when wiring the world's `import
/// mty:web/canvas;` line.
pub const WIT_INTERFACE_CANVAS: &str = "mty:web/canvas@0.1";

/// Canonical WIT interface name for `input`. See
/// [`WIT_INTERFACE_CANVAS`] for the rationale.
pub const WIT_INTERFACE_INPUT: &str = "mty:web/input@0.1";

/// Canonical WIT world name for the canvas-driving agent shape
/// (`world agent { import canvas; import input; import log; … }`).
pub const WIT_WORLD_AGENT: &str = "mty:web/agent@0.1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_names_are_canonical() {
        assert_eq!(WIT_INTERFACE_CANVAS, "mty:web/canvas@0.1");
        assert_eq!(WIT_INTERFACE_INPUT, "mty:web/input@0.1");
        assert_eq!(WIT_WORLD_AGENT, "mty:web/agent@0.1");
    }
}
