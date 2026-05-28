//! Library face of `mty-cli`.
//!
//! The binary entry point lives in `main.rs`; this `lib.rs` exists
//! solely so integration tests under `tests/` can reach into the
//! command modules (e.g. `mty_cli::cmd::find::parse_source_for_tests`).
//! Keep this surface minimal — the public CLI shape is `mty <cmd>`,
//! not the Rust API.

pub mod cmd;
