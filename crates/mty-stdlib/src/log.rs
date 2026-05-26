//! `std.log` — print-shaped output for Mighty's `log()` / `print()`
//! builtins, plus the canonical-import constants the codegen layer
//! pattern-matches on when wiring the WASI Preview 2 direct-lowering
//! sequence (v0.17+).
//!
//! ## Background
//!
//! v0.13–v0.16 routed `log()` through an unversioned
//! `wasi:cli/log#log` shim. `wit-component::ComponentEncoder` resolved
//! the shim against an in-line WIT package declared inside the
//! generated P2 document; at instantiation time the vendored P1→P2
//! adapter translated the call into a real `fd_write`-shaped syscall.
//!
//! v0.17 drops the shim entirely. The codegen layer
//! (`mty_codegen_wasm::P2DirectImport::{LogStdoutGet, LogStreamWrite,
//! LogStreamDrop}`) splices a three-call canonical-ABI sequence at
//! every `log()` site:
//!
//!   1. `handle = wasi:cli/stdout@0.2.3.get-stdout()`
//!   2. `wasi:io/streams@0.2.3.[method]output-stream.blocking-write-and-flush(handle, ptr, len)`
//!   3. `[resource-drop]wasi:io/streams@0.2.3.output-stream(handle)`
//!
//! The constants below are the single source of truth for the import
//! names; both the codegen-wasm crate and any external test harness
//! pattern-match on them.

/// Canonical P2 import name for `wasi:cli/stdout.get-stdout`.
///
/// Emits the handle to the program's standard-output stream (an
/// `own<output-stream>`); the emitter immediately consumes it via
/// [`P2_DIRECT_IMPORT_LOG_STREAM_WRITE`] and then drops the handle
/// with [`P2_DIRECT_IMPORT_LOG_STREAM_DROP`].
pub const P2_DIRECT_IMPORT_LOG_STDOUT_GET: (&str, &str) = ("wasi:cli/stdout@0.2.3", "get-stdout");

/// Canonical P2 import name for the blocking write-and-flush call on
/// a `wasi:io/streams.output-stream` resource.
///
/// The blocking variant matches the historical fire-and-forget
/// semantics of `wasi:cli/log#log` without threading a
/// `wasi:io/poll.pollable` through Mighty's `log()` surface.
pub const P2_DIRECT_IMPORT_LOG_STREAM_WRITE: (&str, &str) = (
    "wasi:io/streams@0.2.3",
    "[method]output-stream.blocking-write-and-flush",
);

/// Canonical resource-drop intrinsic for the `output-stream` handle
/// returned by [`P2_DIRECT_IMPORT_LOG_STDOUT_GET`].
///
/// The emitter calls this after every `log()` so the handle never
/// leaks into the host's resource table.
pub const P2_DIRECT_IMPORT_LOG_STREAM_DROP: (&str, &str) =
    ("wasi:io/streams@0.2.3", "[resource-drop]output-stream");

/// Host-side fallback used by the runtime's `host_std` dispatcher
/// for builds that aren't compiled to wasm (e.g. the JIT path and
/// the native test harness). Writes `msg` to stdout, appending a
/// newline.
///
/// Wasm builds never reach this — the codegen layer rewrites
/// `log(s)` to a direct P2 import sequence (see the module-level
/// docs).
pub fn write_line(msg: &str) {
    // Use `println!` so the output is line-buffered to match the
    // `[method]output-stream.blocking-write-and-flush` semantics on
    // the wasm path (the flush is implicit in `println!`'s newline +
    // periodic-flush behaviour).
    println!("{msg}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_p2_constants_are_canonical() {
        // Namespace + version must match the rest of the P2 0.2.3
        // surface (kept in lockstep with
        // `mty_codegen_wasm::WASI_P2_VERSION`).
        assert_eq!(P2_DIRECT_IMPORT_LOG_STDOUT_GET.0, "wasi:cli/stdout@0.2.3");
        assert_eq!(P2_DIRECT_IMPORT_LOG_STDOUT_GET.1, "get-stdout");
        assert_eq!(P2_DIRECT_IMPORT_LOG_STREAM_WRITE.0, "wasi:io/streams@0.2.3");
        assert!(P2_DIRECT_IMPORT_LOG_STREAM_WRITE
            .1
            .starts_with("[method]output-stream."));
        assert!(P2_DIRECT_IMPORT_LOG_STREAM_WRITE
            .1
            .ends_with("blocking-write-and-flush"));
        assert_eq!(P2_DIRECT_IMPORT_LOG_STREAM_DROP.0, "wasi:io/streams@0.2.3");
        assert_eq!(
            P2_DIRECT_IMPORT_LOG_STREAM_DROP.1,
            "[resource-drop]output-stream"
        );
    }

    #[test]
    fn log_write_line_doesnt_panic() {
        write_line("hello from std.log host fallback");
    }
}
