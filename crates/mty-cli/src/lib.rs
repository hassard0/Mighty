//! Library face of `mty-cli`.
//!
//! The binary entry point lives in `main.rs`; this `lib.rs` exists
//! so integration tests under `tests/` can reach into the command
//! modules (e.g. `mty_cli::cmd::find::parse_source_for_tests`) AND
//! so `wasm-pack` has a `cdylib` to attach the browser playground's
//! wasm-bindgen exports to.
//!
//! Keep this surface minimal — the public CLI shape is `mty <cmd>`,
//! not the Rust API.
//
// v0.35 T1 — the `cmd` tree pulls in tokio/hyper/codegen/runtime, so
// it's gated behind the default-on `host-toolchain` feature. The
// `playground-wasm` build (wasm32-unknown-unknown) only needs the
// `playground` module's exports + the lightweight parse/typeck
// surface re-exported by `mty-driver` (also gated).

#[cfg(feature = "host-toolchain")]
pub mod cmd;

// v0.35 T1 — browser playground exports. `wasm-pack build --target web`
// reads this cdylib's `#[wasm_bindgen]` symbols (`init` / `check` /
// `run`) and emits the JS glue under
// `tools/playground/public/wasm/mty_cli.js` + `mty_cli_bg.wasm`. See
// `src/playground.rs` and `tools/playground/src/runner.ts`.
#[cfg(all(target_arch = "wasm32", feature = "playground-wasm"))]
pub mod playground;
