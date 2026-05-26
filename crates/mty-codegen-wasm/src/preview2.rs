//! WASI Preview 2 (0.2.x) host bindings + component wrapping.
//!
//! v0.13 introduces an opt-in P2 path for the Wasm Component Model
//! backend. The existing P1 pipeline (`emit_wit` + `wrap_as_component`)
//! is untouched and remains the default; callers ask for P2 via
//! [`Preview2Options`] (driven by the `--wasi=p2` CLI flag).
//!
//! ### What the P2 path produces
//!
//! Given the same core Wasm module the slice-8 emitter produces, this
//! module builds a Component Model component whose imports are the
//! versioned WASI P2 interface set:
//!
//! - `wasi:cli@0.2.3` (stdout/stderr/stdin/exit/environment)
//! - `wasi:io@0.2.3`  (streams + poll + error)
//! - `wasi:clocks@0.2.3` (monotonic + wall-clock)
//! - `wasi:filesystem@0.2.3` (preopens + descriptor types)
//! - `wasi:http@0.2.3` (outgoing-handler + request/response types)
//! - `wasi:random@0.2.3`
//!
//! The core module's existing `wasi:cli/log#log` import is preserved as
//! a Mighty-internal *adapter import* — the P2 world declares the same
//! interface alongside the P2 ones so the existing core module
//! continues to validate. A v0.14 follow-up will replace the adapter
//! with a real `wasi:cli/stdout#print` lowering.
//!
//! ### User-WIT integration
//!
//! When the caller supplies a user package world (loaded via
//! `mty_pkg::wit_resolve::UserWit`), it is merged into the generated
//! WIT document so the user's exports / additional imports show up in
//! the produced component. The user world's *name* takes precedence
//! over the synthesized `<pkg>-world` name.
//!
//! ### What's stubbed
//!
//! `wasi:filesystem@0.2.3` resource methods (descriptor `read-via-stream`
//! etc.) are declared but not wired through from `std.fs`. Calls to
//! `std.fs.read` still lower to the P1 import shape; before v0.17 the
//! component embedded a vendored `wasi_snapshot_preview1` adapter built
//! from wasmtime v32.0.0 so the P1-shaped calls could be translated to
//! versioned P2 imports at instantiation time. v0.17 flipped the default
//! to `None` (every stdlib call now has a direct P2 lowering) and v0.19
//! removed the vendored adapter bytes entirely — callers that still need
//! to link wasi-libc C code can supply their own bytes via
//! [`Preview2Options::with_adapter`]. The boundary is documented in
//! `docs/reference/wasi.md` and `WASI_P2_LOWERINGS_V0_14_NOTES.md`.
//!
//! ### v0.14 direct-import lowering helpers
//!
//! For `std.random.*` and `std.time.*` we additionally expose direct
//! P2 import-emission helpers ([`emit_p2_random_bytes_import`],
//! [`emit_p2_monotonic_clock_now_import`], …) that callers can splice
//! into a core module under construction. These helpers mint the
//! versioned import shape `wasi:random/random@0.2.3#get-random-bytes`
//! (et al.) so the core module imports the canonical P2 interface
//! directly, bypassing the adapter for those cases where the
//! `wasi-libc`-generated calls would otherwise route through it.
//! `std.fs` and `std.http` continue to use the adapter — their
//! resource-typed surfaces require richer canonical-ABI plumbing
//! that's tracked for v0.15.

use crate::emit::{compile_program_to_bytes_with_preview, EmitWasiPreview};
use crate::error::{CompileResult, WasmError};
use crate::target::WasmTarget;
use crate::wit::WitDocument;
use mty_ir::ir::Program;

/// Vendored, in-tree slice of WASI Preview 2 (0.2.3). See
/// `crates/mty-codegen-wasm/wit/wasi-p2/wasi-p2.wit` for the source.
///
/// This is loaded into every P2 build's `wit_parser::Resolve` so user
/// worlds can `use` from the P2 namespaces without a vendored `wit/deps`
/// tree on disk.
pub const VENDORED_WASI_P2_WIT: &str = include_str!("../wit/wasi-p2/wasi-p2.wit");

/// The WASI P2 version Mighty v0.13 targets.
pub const WASI_P2_VERSION: &str = "0.2.3";

/// Which P1→P2 adapter shape a caller is supplying. Driven by the
/// component "kind" the build produces (command/reactor/proxy).
/// `mty build` only emits commands today so [`AdapterKind::Command`]
/// is the most common pick.
///
/// As of v0.19 Mighty no longer vendors any adapter bytes — the kind
/// enum is kept so the wrapping path can decide whether to synthesize
/// the `_start` alias required for command-shape adapters. Callers
/// that want adapter wrapping supply their own bytes via
/// [`Preview2Options::with_adapter`] (typically downloaded from the
/// matching wasmtime release for their target WASI version).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    /// `wasi_snapshot_preview1.command.wasm` — programs with `main`.
    Command,
    /// `wasi_snapshot_preview1.reactor.wasm` — exported-function
    /// libraries (no `main`).
    Reactor,
    /// `wasi_snapshot_preview1.proxy.wasm` — wasi-http proxy shape.
    Proxy,
}

impl AdapterKind {
    /// Human-readable name (matches the upstream file stem). Used by
    /// the `wit-component` adapter API — the same string the core
    /// module's imports name (`wasi_snapshot_preview1`).
    pub fn import_module_name(self) -> &'static str {
        // All three adapter shapes export the same legacy module
        // name; the difference is in what they import and how they
        // instantiate.
        "wasi_snapshot_preview1"
    }
}

/// Caller-supplied P1→P2 adapter bytes plus the kind they correspond
/// to. v0.19 no longer ships vendored adapter bytes in-tree — programs
/// that need adapter wrapping (typically because they bundle a
/// wasi-libc-built C crate) download the matching adapter from the
/// upstream wasmtime release and pass it here.
///
/// The [`AdapterKind`] tag drives the small bit of conditional logic
/// in [`wrap_p2`] that aliases `main` as `_start` for command-shape
/// adapters (the wasmtime command adapter's `wasi:cli/run.run`
/// re-export looks for a `_start` export).
#[derive(Debug, Clone)]
pub struct AdapterEmbed {
    /// Which adapter shape these bytes correspond to. Drives
    /// `_start`-alias scaffolding in the wrapper.
    pub kind: AdapterKind,
    /// The raw `wasi_snapshot_preview1.*.wasm` bytes the caller wants
    /// `wit-component` to embed.
    pub bytes: Vec<u8>,
}

impl AdapterEmbed {
    /// Convenience constructor — same field order as the struct.
    pub fn new(kind: AdapterKind, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            kind,
            bytes: bytes.into(),
        }
    }
}

/// v0.14+ stdlib-direct lowering descriptors. Each variant names a
/// versioned P2 import the codegen layer can splice into a core
/// module under construction, in place of an equivalent P1 syscall.
///
/// Kept as a flat enum rather than free functions so callers can
/// pattern-match for tests + diagnostics without coupling to the
/// specific `wasm-encoder` types.
///
/// ### v0.16 — fs + http resource-shape lowerings
///
/// The `Fs*` and `Http*` variants added in v0.16 target imports
/// whose canonical-ABI shapes carry **resource handles** (i32 at
/// the core-Wasm boundary) and pass strings / records through
/// return-area pointers. Helpers
/// [`canonical_abi_descriptor_signature`] /
/// [`canonical_abi_outgoing_request_signature`] document each
/// shape next to the matching variant so the emitter and tests
/// agree on the wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum P2DirectImport {
    /// `wasi:random/random@0.2.3#get-random-bytes(len: u64) -> list<u8>`
    /// — the canonical "secure random bytes" call. Mighty's
    /// `std.random.bytes(n)` lowers to this on `--wasi=p2`.
    RandomBytes,
    /// `wasi:clocks/monotonic-clock@0.2.3#now() -> instant`
    /// — monotonic instant in nanoseconds since an unspecified epoch.
    /// Mighty's `std.time.monotonic_now()` lowers to this on
    /// `--wasi=p2`.
    MonotonicNow,
    /// `wasi:clocks/wall-clock@0.2.3#now() -> datetime`
    /// — wall-clock instant (seconds + nanos since UNIX epoch).
    /// Mighty's `std.time.now()` lowers to this on `--wasi=p2`.
    WallClockNow,
    /// `wasi:clocks/monotonic-clock@0.2.3#resolution() -> instant`
    /// — resolution of the monotonic clock in nanoseconds. Mighty's
    /// `std.time.resolution()` lowers to this on `--wasi=p2`.
    MonotonicResolution,
    // ---------------------------------------------------------------
    // v0.16 — filesystem direct lowerings.
    //
    // `descriptor` is a resource; at the canonical-ABI boundary it
    // shows up as an `i32` handle. All the methods below are method
    // calls on `borrow<descriptor>` (lowered as `(self_handle: i32, …)`
    // at the core-Wasm boundary). Results that carry an owned
    // resource (`descriptor` from `open-at`) are written to a
    // return-area as `(tag: i32, handle_or_err: i32)`.
    // ---------------------------------------------------------------
    /// `wasi:filesystem/types@0.2.3.descriptor.open-at(
    ///     path-flags: path-flags, path: string,
    ///     open-flags: open-flags, %flags: descriptor-flags
    /// ) -> result<descriptor, error-code>`.
    ///
    /// Mighty's `std.fs.open(path)` lowers to this on `--wasi=p2`.
    /// Core-Wasm: `(self:i32, path-flags:i32, path-ptr:i32, path-len:i32,
    /// open-flags:i32, desc-flags:i32, ret-area:i32) -> ()`.
    FsOpenAt,
    /// `wasi:filesystem/types@0.2.3.descriptor.read-via-stream(
    ///     offset: filesize
    /// ) -> result<input-stream, error-code>`.
    ///
    /// Mighty's `std.fs.read_file(path)` lowers to a 3-step sequence:
    /// open-at → read-via-stream → stream consume. This variant
    /// represents the middle step (the only one whose import index
    /// the emitter splices into the import section per-call).
    /// Core-Wasm: `(self:i32, offset:i64, ret-area:i32) -> ()`.
    FsReadViaStream,
    /// `wasi:filesystem/types@0.2.3.descriptor.write-via-stream(
    ///     offset: filesize
    /// ) -> result<output-stream, error-code>`.
    ///
    /// Mighty's `std.fs.write_file(path, data)` lowers via
    /// open-at → write-via-stream → stream blocking-write-and-flush.
    /// Core-Wasm: `(self:i32, offset:i64, ret-area:i32) -> ()`.
    FsWriteViaStream,
    /// `wasi:filesystem/types@0.2.3.descriptor.stat() -> result<descriptor-stat, error-code>`.
    ///
    /// Mighty's `std.fs.stat(path)` lowers to open-at → stat → close.
    /// Core-Wasm: `(self:i32, ret-area:i32) -> ()`. The ret-area
    /// holds `(tag: i32, descriptor-stat | error-code)`; the
    /// `descriptor-stat` record is laid out as documented in
    /// [`canonical_abi_descriptor_stat_layout`].
    FsStat,
    /// `[resource-drop]wasi:filesystem/types@0.2.3.descriptor`.
    ///
    /// The Component Model's resource-drop intrinsic — when the host
    /// no longer needs an `own<descriptor>`, the core module calls
    /// this to release the handle. Mighty's `std.fs.close(handle)`
    /// lowers to this directly.
    /// Core-Wasm: `(self:i32) -> ()`.
    FsClose,
    // ---------------------------------------------------------------
    // v0.16 — http direct lowerings.
    //
    // The wasi-http surface is resource-heavy. We expose four
    // "spine" calls covering the common GET / POST request
    // lifecycle. Mighty's `std.http.get(url)` lowers as:
    //
    //   1. headers = fields::new()           (resource constructor)
    //   2. req = new-outgoing-request(headers)
    //   3. resp_fut = outgoing-handler::handle(req, none)
    //   4. resp = future.get().unwrap()       (blocks via subscribe)
    //   5. status = resp.status()
    //   6. body = resp.consume()              (incoming-body stream)
    //
    // The emitter splices steps 2-5 (the only ones with a top-level
    // canonical-ABI import index) and uses scratch return-areas for
    // each step's i32 handle output.
    // ---------------------------------------------------------------
    /// `wasi:http/types@0.2.3#new-outgoing-request(headers: headers) -> outgoing-request`.
    ///
    /// Free function (not a method) that wraps the
    /// `outgoing-request` resource constructor.
    /// Core-Wasm: `(headers:i32) -> i32`.
    HttpNewRequest,
    /// `wasi:http/outgoing-handler@0.2.3#handle(
    ///     request: outgoing-request,
    ///     options: option<request-options>
    /// ) -> result<future-incoming-response, error-code>`.
    ///
    /// The blocking-style "send this request" call. Core-Wasm:
    /// `(req:i32, opt-tag:i32, opt-handle:i32, ret-area:i32) -> ()`.
    HttpHandleRequest,
    /// `wasi:http/types@0.2.3.incoming-response.status() -> status-code`.
    ///
    /// `status-code` is a `u16` so the core-Wasm shape is
    /// `(self:i32) -> i32`.
    HttpResponseStatus,
    /// `wasi:http/types@0.2.3.incoming-response.consume() -> result<incoming-body>`.
    ///
    /// Hands ownership of the body to the caller. Core-Wasm:
    /// `(self:i32, ret-area:i32) -> ()`.
    HttpResponseBody,
    /// v0.17 — `wasi:cli/stdout@0.2.3#get-stdout() -> output-stream`.
    /// Free function (not a method). Returns an owned
    /// `output-stream` resource handle. Mighty's slice-8 emitter
    /// calls this at every `log()` site to acquire stdout.
    /// Core-Wasm: `() -> i32`.
    LogStdoutGet,
    /// v0.17 — `wasi:io/streams@0.2.3.[method]output-stream.blocking-write-and-flush(
    ///     self: borrow<output-stream>, contents: list<u8>
    /// ) -> result<_, stream-error>`.
    /// Core-Wasm: `(self:i32, ptr:i32, len:i32, ret-area:i32) -> ()`.
    /// Mighty discards the result for `log()` — the slice-8 builtin
    /// has no `Result` return.
    LogStreamWrite,
    /// v0.17 — `[resource-drop]wasi:io/streams@0.2.3.output-stream`.
    /// Required after every `get-stdout` so the host's resource
    /// table doesn't fill up with one entry per `log()` call.
    /// Core-Wasm: `(self:i32) -> ()`.
    LogStreamDrop,
}

impl P2DirectImport {
    /// The `(module_name, fn_name)` pair as it appears in the core
    /// Wasm module's import section. Module names match the WIT
    /// "namespace/interface@version" form that
    /// `wit-component::ComponentEncoder` lifts into the P2 component
    /// without an adapter hop.
    pub fn import_pair(self) -> (&'static str, &'static str) {
        match self {
            P2DirectImport::RandomBytes => ("wasi:random/random@0.2.3", "get-random-bytes"),
            P2DirectImport::MonotonicNow => ("wasi:clocks/monotonic-clock@0.2.3", "now"),
            P2DirectImport::WallClockNow => ("wasi:clocks/wall-clock@0.2.3", "now"),
            P2DirectImport::MonotonicResolution => {
                ("wasi:clocks/monotonic-clock@0.2.3", "resolution")
            }
            // v0.16 — filesystem.
            //
            // Resource-method imports are emitted by wit-component as
            // `<interface>#[method]<resource>.<name>`. The same name
            // shape goes into the core module's import section so
            // the encoder can wire it up.
            P2DirectImport::FsOpenAt => {
                ("wasi:filesystem/types@0.2.3", "[method]descriptor.open-at")
            }
            P2DirectImport::FsReadViaStream => (
                "wasi:filesystem/types@0.2.3",
                "[method]descriptor.read-via-stream",
            ),
            P2DirectImport::FsWriteViaStream => (
                "wasi:filesystem/types@0.2.3",
                "[method]descriptor.write-via-stream",
            ),
            P2DirectImport::FsStat => ("wasi:filesystem/types@0.2.3", "[method]descriptor.stat"),
            P2DirectImport::FsClose => ("wasi:filesystem/types@0.2.3", "[resource-drop]descriptor"),
            // v0.16 — http.
            P2DirectImport::HttpNewRequest => {
                ("wasi:http/types@0.2.3", "[constructor]outgoing-request")
            }
            P2DirectImport::HttpHandleRequest => ("wasi:http/outgoing-handler@0.2.3", "handle"),
            P2DirectImport::HttpResponseStatus => {
                ("wasi:http/types@0.2.3", "[method]incoming-response.status")
            }
            P2DirectImport::HttpResponseBody => {
                ("wasi:http/types@0.2.3", "[method]incoming-response.consume")
            }
            // v0.17 — log() direct lowering.
            P2DirectImport::LogStdoutGet => ("wasi:cli/stdout@0.2.3", "get-stdout"),
            P2DirectImport::LogStreamWrite => (
                "wasi:io/streams@0.2.3",
                "[method]output-stream.blocking-write-and-flush",
            ),
            P2DirectImport::LogStreamDrop => {
                ("wasi:io/streams@0.2.3", "[resource-drop]output-stream")
            }
        }
    }

    /// Stable name used in diagnostics, test assertions and the
    /// `Display` impl. Matches the variant ident in snake-case.
    pub fn label(self) -> &'static str {
        match self {
            P2DirectImport::RandomBytes => "random_bytes",
            P2DirectImport::MonotonicNow => "monotonic_now",
            P2DirectImport::WallClockNow => "wall_clock_now",
            P2DirectImport::MonotonicResolution => "monotonic_resolution",
            P2DirectImport::FsOpenAt => "fs_open_at",
            P2DirectImport::FsReadViaStream => "fs_read_via_stream",
            P2DirectImport::FsWriteViaStream => "fs_write_via_stream",
            P2DirectImport::FsStat => "fs_stat",
            P2DirectImport::FsClose => "fs_close",
            P2DirectImport::HttpNewRequest => "http_new_request",
            P2DirectImport::HttpHandleRequest => "http_handle_request",
            P2DirectImport::HttpResponseStatus => "http_response_status",
            P2DirectImport::HttpResponseBody => "http_response_body",
            P2DirectImport::LogStdoutGet => "log_stdout_get",
            P2DirectImport::LogStreamWrite => "log_stream_write",
            P2DirectImport::LogStreamDrop => "log_stream_drop",
        }
    }

    /// Returns `true` iff this variant references a wasi:filesystem
    /// resource method (anything in the `Fs*` family). Used by the
    /// emitter to decide whether the call needs an extra
    /// resource-borrow / drop scaffold around it.
    pub fn is_filesystem(self) -> bool {
        matches!(
            self,
            P2DirectImport::FsOpenAt
                | P2DirectImport::FsReadViaStream
                | P2DirectImport::FsWriteViaStream
                | P2DirectImport::FsStat
                | P2DirectImport::FsClose
        )
    }

    /// Returns `true` iff this variant references a wasi:http call.
    pub fn is_http(self) -> bool {
        matches!(
            self,
            P2DirectImport::HttpNewRequest
                | P2DirectImport::HttpHandleRequest
                | P2DirectImport::HttpResponseStatus
                | P2DirectImport::HttpResponseBody
        )
    }

    /// v0.17 — returns `true` iff this variant is one of the
    /// `log()` direct-lowering imports (the trio that replaces
    /// the v0.13 `wasi:cli/log` shim).
    pub fn is_log(self) -> bool {
        matches!(
            self,
            P2DirectImport::LogStdoutGet
                | P2DirectImport::LogStreamWrite
                | P2DirectImport::LogStreamDrop
        )
    }
}

/// Canonical-ABI core-Wasm signature for a `borrow<descriptor>`-shaped
/// `wasi:filesystem` resource method. Returned as `(params, results)`
/// where the leading `i32` is the resource-handle (the implicit
/// `self`). See [`P2DirectImport`] doc-comments for per-variant
/// layouts.
pub fn canonical_abi_descriptor_signature(
    which: P2DirectImport,
) -> (Vec<wasm_encoder::ValType>, Vec<wasm_encoder::ValType>) {
    use wasm_encoder::ValType::{I32, I64};
    match which {
        // (self, path-flags, path-ptr, path-len, open-flags, desc-flags, ret-area) -> ()
        P2DirectImport::FsOpenAt => (vec![I32, I32, I32, I32, I32, I32, I32], vec![]),
        // (self, offset:i64, ret-area:i32) -> ()
        P2DirectImport::FsReadViaStream | P2DirectImport::FsWriteViaStream => {
            (vec![I32, I64, I32], vec![])
        }
        // (self, ret-area:i32) -> ()
        P2DirectImport::FsStat => (vec![I32, I32], vec![]),
        // (self) -> ()  — resource-drop intrinsic shape
        P2DirectImport::FsClose => (vec![I32], vec![]),
        _ => panic!("canonical_abi_descriptor_signature: not a filesystem variant: {which:?}"),
    }
}

/// Canonical-ABI core-Wasm signature for the wasi-http calls Mighty
/// v0.16 lowers directly. See [`P2DirectImport::Http*`] doc-comments
/// for the per-variant layouts.
pub fn canonical_abi_outgoing_request_signature(
    which: P2DirectImport,
) -> (Vec<wasm_encoder::ValType>, Vec<wasm_encoder::ValType>) {
    use wasm_encoder::ValType::I32;
    match which {
        // [constructor]outgoing-request(headers: headers) -> outgoing-request
        //   → core: (headers:i32) -> i32
        P2DirectImport::HttpNewRequest => (vec![I32], vec![I32]),
        // handle(request, option<request-options>) -> result<future-incoming-response, error-code>
        //   → core: (req:i32, opt-tag:i32, opt-handle:i32, ret-area:i32) -> ()
        P2DirectImport::HttpHandleRequest => (vec![I32, I32, I32, I32], vec![]),
        // [method]incoming-response.status() -> status-code
        //   → core: (self:i32) -> i32
        P2DirectImport::HttpResponseStatus => (vec![I32], vec![I32]),
        // [method]incoming-response.consume() -> result<incoming-body>
        //   → core: (self:i32, ret-area:i32) -> ()
        P2DirectImport::HttpResponseBody => (vec![I32, I32], vec![]),
        _ => panic!("canonical_abi_outgoing_request_signature: not an http variant: {which:?}"),
    }
}

/// Canonical-ABI return-area layout for a `descriptor-stat` record
/// (used by `descriptor.stat`). Documented as a constant block so
/// the emitter, tests, and host can agree on the field offsets.
///
/// Layout (all little-endian, naturally aligned):
///
/// | Offset | Width | Field                            |
/// |--------|-------|----------------------------------|
/// |   0    |   1   | result tag (0 = ok, 1 = err)     |
/// |   8    |   8   | record `descriptor-stat`:        |
/// |   8    |   1   |   `type` (variant `descriptor-type`) |
/// |  16    |   8   |   `link-count` (u64)             |
/// |  24    |   8   |   `size` (u64)                   |
/// |  32    |   8   |   `data-access-timestamp.seconds` (u64) |
/// |  40    |   4   |   `data-access-timestamp.nanoseconds` (u32) |
/// |  48    |   8   |   `data-modification-timestamp.seconds` (u64) |
/// |  56    |   4   |   `data-modification-timestamp.nanoseconds` (u32) |
/// |  64    |   8   |   `status-change-timestamp.seconds` (u64) |
/// |  72    |   4   |   `status-change-timestamp.nanoseconds` (u32) |
///
/// Total: 80 bytes. The emitter reserves the same-size block at
/// [`DOM_RETURN_AREA`] (which already has plenty of headroom).
pub const CANONICAL_ABI_DESCRIPTOR_STAT_SIZE: usize = 80;

/// Emit a `resource-drop` call for a `wasi:filesystem` descriptor
/// handle currently on the wasm operand stack. Helper kept here so
/// callers in the codegen layer don't reimplement the canonical-ABI
/// drop convention; equivalent to splicing the [`P2DirectImport::FsClose`]
/// import and emitting `call <fs_close_idx>`.
///
/// The caller is responsible for having the descriptor handle (an
/// `i32`) on the stack when this is invoked, and for having declared
/// the import via [`P2DirectImport::FsClose`]'s `import_pair()`.
pub fn emit_resource_drop_call(builder: &mut wasm_encoder::Function, drop_fn_idx: u32) {
    builder.instruction(&wasm_encoder::Instruction::Call(drop_fn_idx));
}

/// Companion helper to [`emit_resource_drop_call`] for the "borrow"
/// side — a `borrow<descriptor>` at the canonical-ABI is just an
/// `i32` handle, so the helper is a no-op that documents intent.
/// Kept so the emitter's call sites read self-documenting.
pub fn emit_resource_borrow_passthrough(_handle_local: u32) {
    // `borrow<resource>` is a transparent handle at the core-Wasm
    // boundary. The caller already loaded the handle local before
    // calling the import — no extra instruction needed.
}

impl std::fmt::Display for P2DirectImport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (m, n) = self.import_pair();
        write!(f, "{m}#{n}")
    }
}

/// v0.17 — emit the canonical-ABI call sequence for a `log()`
/// call targeting the direct P2 stdout lowering.
///
/// The sequence is:
///
/// 1. `call $get_stdout`                ;; output-stream handle (i32)
/// 2. `local.tee $handle_local`         ;; stash handle in a local
/// 3. `i32.const $msg_ptr`              ;; bytes ptr
/// 4. `i32.const $msg_len`              ;; bytes len
/// 5. `i32.const $ret_area`             ;; result return-area
/// 6. `call $blocking_write_and_flush`  ;; (self, ptr, len, ret) -> ()
/// 7. `local.get $handle_local`         ;; reload handle
/// 8. `call $stream_drop`               ;; release stream handle
///
/// `blocking-write-and-flush` returns a `result<_, stream-error>`
/// (canonical-ABI shape `(tag:i32, err-handle:i32)` in the
/// supplied ret-area). For `log()` we deliberately discard it —
/// the slice-8 `log()` builtin has no `Result` return and the
/// historical `wasi:cli/log#log` shim was equally fire-and-forget.
/// The ret-area memory still has to be supplied so the canonical
/// ABI doesn't trap on a null pointer.
///
/// Callers are responsible for:
///   * Having declared the three imports
///     ([`P2DirectImport::LogStdoutGet`],
///     [`P2DirectImport::LogStreamWrite`],
///     [`P2DirectImport::LogStreamDrop`]) and passing their
///     function indices.
///   * Having reserved an `i32` local slot for the stream handle.
///   * Having interned the message bytes into linear memory and
///     passing their `(ptr, len)`.
///   * Supplying a `ret_area` pointer — Mighty's emitter passes
///     [`crate::emit::LOG_RETURN_AREA`].
#[allow(clippy::too_many_arguments)]
pub fn emit_log_call_sequence(
    builder: &mut wasm_encoder::Function,
    stdout_get_idx: u32,
    write_idx: u32,
    drop_idx: u32,
    handle_local: u32,
    msg_ptr: u32,
    msg_len: u32,
    ret_area: u32,
) {
    use wasm_encoder::Instruction as I;
    // 1. get-stdout() -> output-stream handle (i32).
    builder.instruction(&I::Call(stdout_get_idx));
    // 2. tee the handle into the dedicated i32 local. local.tee
    //    leaves the value on the operand stack AND stores it in
    //    the local, so the handle becomes the first arg for the
    //    upcoming write call.
    builder.instruction(&I::LocalTee(handle_local));
    // 3-6. blocking-write-and-flush(self_handle, ptr, len, ret_area)
    builder.instruction(&I::I32Const(msg_ptr as i32));
    builder.instruction(&I::I32Const(msg_len as i32));
    builder.instruction(&I::I32Const(ret_area as i32));
    builder.instruction(&I::Call(write_idx));
    // 7-8. Drop the stream handle on the way out.
    builder.instruction(&I::LocalGet(handle_local));
    builder.instruction(&I::Call(drop_idx));
}

/// Per-build options for the WASI Preview 2 backend.
#[derive(Debug, Clone)]
pub struct Preview2Options {
    /// Package name (kebab-case stem). Used for the synthesized
    /// `<pkg>-world` when no user-world is supplied.
    pub pkg_name: String,
    /// Optional user-supplied WIT (parsed + resolved by
    /// `mty_pkg::wit_resolve`). When `Some`, the user's package is
    /// merged into the component's exported world.
    pub user_wit: Option<UserWit>,
    /// Caller-supplied P1→P2 adapter to embed when wrapping. v0.17
    /// default: `None` — every stdlib call lowers directly to a
    /// versioned P2 import so no adapter hop is required.
    ///
    /// Set to `Some(AdapterEmbed { kind, bytes })` when the core
    /// module bundles a `wasi-libc`-compiled C crate or otherwise
    /// imports `wasi_snapshot_preview1` directly. The bytes are
    /// caller-supplied because v0.19 no longer vendors the adapter
    /// in-tree (the default path doesn't need it and we don't want
    /// every Mighty consumer paying the ~150 KB cost). Sources:
    /// the matching wasmtime release's
    /// `wasi_snapshot_preview1.{command,reactor,proxy}.wasm`.
    pub embed_adapter: Option<AdapterEmbed>,
}

/// A user-authored WIT package, pre-loaded by the caller (typically
/// `mty_pkg::wit_resolve::load_user_wit`).
///
/// We accept the *raw text* of the user's package so that
/// `preview2.rs` is the single source of truth for `wit_parser::Resolve`
/// composition. This keeps the user-WIT loader (`mty-pkg`) decoupled
/// from `wit-parser`'s API at the type level.
#[derive(Debug, Clone)]
pub struct UserWit {
    /// Concatenated text of every user `.wit` file (with package
    /// declarations preserved). Loaded by `mty_pkg::wit_resolve`.
    pub text: String,
    /// Optional explicit world name (`--world <name>`). When `None`
    /// we pick the *only* world in the user's package; if there are
    /// multiple, we surface a [`WasmError::Invalid`].
    pub world: Option<String>,
    /// Source label used in `wit_parser` diagnostics.
    pub source_label: String,
}

impl Preview2Options {
    /// Construct a new Preview2Options with the v0.17 defaults:
    /// no embedded P1→P2 adapter (`embed_adapter = None`), no
    /// user WIT. The default flipped in v0.17 because the codegen
    /// layer now emits direct P2 lowerings for every stdlib call
    /// it touches (random + time since v0.15, fs + http since
    /// v0.16, log since v0.17) — programs that don't reach for
    /// legacy P1 syscalls ship adapter-free. Opt back in via
    /// [`Self::with_adapter`] when linking wasi-libc-built C code.
    pub fn new(pkg_name: impl Into<String>) -> Self {
        Self {
            pkg_name: pkg_name.into(),
            user_wit: None,
            embed_adapter: None,
        }
    }

    pub fn with_user_wit(mut self, uw: UserWit) -> Self {
        self.user_wit = Some(uw);
        self
    }

    /// Override the embedded adapter.
    ///
    /// `None` (the v0.17 default) skips adapter embedding — safe
    /// when the core module exclusively imports versioned P2
    /// interfaces (true for any all-Mighty program since v0.17).
    /// `Some(AdapterEmbed::new(AdapterKind::Command, bytes))`
    /// reattaches a caller-supplied adapter; pre-v0.19 builds got
    /// the bytes from the in-tree
    /// `crates/mty-codegen-wasm/adapter/` directory, v0.19+ callers
    /// download the matching wasmtime release and pass the bytes
    /// here.
    pub fn with_adapter(mut self, adapter: Option<AdapterEmbed>) -> Self {
        self.embed_adapter = adapter;
        self
    }
}

/// Build the P2 WIT document. This is exposed for testing; the normal
/// build path goes through [`compile_program_to_bytes_p2`] /
/// [`compile_program_to_file_p2`].
pub fn emit_wit_p2(_prog: &Program, opts: &Preview2Options) -> CompileResult<WitDocument> {
    // `_prog` is reserved for v0.14 — once the lowering pass starts
    // synthesizing per-fn exports inside the P2 world (rather than
    // the hard-coded `main`), we'll consume the program's signature
    // here. Keeping the param in the public signature now avoids a
    // breaking change later.
    let pkg_id = sanitize_pkg_id(&opts.pkg_name);

    // Phase 1: synthesize the Mighty package world. We emit it as a
    // self-contained package text so `wit_parser::Resolve` can re-parse
    // it for round-trip validation (matching the v0.2 contract).
    let synth_world_name = format!("{}-world", pkg_id);
    let user_world_name = opts.user_wit.as_ref().and_then(|u| u.world.clone());

    let world_name = user_world_name
        .clone()
        .unwrap_or_else(|| synth_world_name.clone());

    // We assemble the WIT document as a single text blob (returned in
    // [`WitDocument::text`] for callers that want to display it), but
    // we feed `wit_parser::Resolve` *separate* package files so
    // each top-level `package X { ... }` can cross-reference the
    // others (the parser only allows cross-package references between
    // distinct top-level files, not between nested packages in a
    // single file).
    let mut user_body = String::new();
    user_body.push_str("// AUTO-GENERATED by mty-codegen-wasm (WASI Preview 2 path).\n");
    user_body.push_str(&format!(
        "// Target: wasm32-wasi, WASI version: {}.\n",
        WASI_P2_VERSION
    ));
    user_body.push_str(
        "// Adapter: caller-supplied (none by default since v0.17; bytes\n\
         // are no longer vendored in-tree as of v0.19).\n\n",
    );

    // -- 1. The synthesized Mighty package + its primary world. This
    //       must be a *top-level* `package X:Y;` so the resolver
    //       returns it as the document's primary package.
    let mighty_pkg_text = format!(
        "package mighty:{pkg_id};\n\n\
         world {synth_world_name} {{\n\
         {imports}\
           export main: func();\n\
         }}\n",
        imports = synth_world_imports(),
    );

    // -- 2. (v0.13–v0.16) The unversioned `wasi:cli` shim used to
    //       sit here, carrying only the `log` interface so
    //       `wit-component` could resolve the core module's literal
    //       `wasi:cli/log#log` import. v0.17 drops the shim
    //       entirely: the emitter now lowers `log()` directly to a
    //       three-call canonical-ABI sequence on top of
    //       `wasi:cli/stdout@0.2.3` + `wasi:io/streams@0.2.3` (see
    //       [`emit_log_call_sequence`]). The core module no longer
    //       imports `wasi:cli/log`, so the matching WIT package
    //       isn't synthesized either.
    //
    //       Leaving a breadcrumb here so anyone re-reading the
    //       v0.13–v0.16 history sees the migration plan inline.

    // For display + assertions: the public `WitDocument::text` field
    // concatenates everything so test/console pretty-printing still
    // sees one blob.
    user_body.push_str(&mighty_pkg_text);
    user_body.push('\n');
    user_body.push_str("// NOTE: v0.17 dropped the wasi:cli/log shim — log() now\n");
    user_body.push_str("// lowers directly to wasi:cli/stdout@0.2.3 +\n");
    user_body.push_str("// wasi:io/streams@0.2.3#output-stream.blocking-write-and-flush.\n");
    user_body.push('\n');
    user_body.push_str("// ---- Vendored WASI Preview 2 surface (0.2.3) ----\n");
    user_body.push_str(VENDORED_WASI_P2_WIT);
    user_body.push('\n');

    // Phase 2: merge the user's WIT, if any. The user's text may use
    // top-level `package X:Y;` form (canonical when authored as a
    // standalone .wit file).
    if let Some(uw) = &opts.user_wit {
        user_body.push_str("\n// ---- USER-SUPPLIED WIT BELOW ----\n");
        user_body.push_str(&uw.text);
        user_body.push('\n');
    }

    // Round-trip validation — push each top-level package as its own
    // file so they can cross-reference. The vendored P2 slice already
    // contains *nested* packages (one file with multiple
    // `package wasi:X@0.2.3 { ... }` blocks); we have to split it on
    // package-block boundaries first.
    let mut resolve = wit_parser::Resolve::default();
    for (label, pkg_text) in split_nested_into_packages(VENDORED_WASI_P2_WIT, "vendored-p2") {
        let _ = resolve
            .push_str(&label, &pkg_text)
            .map_err(|e| WasmError::Invalid(format!("wit p2 round-trip vendored: {e:#}")))?;
    }
    // v0.17: cli/log shim removed — log() now lowers directly to
    // wasi:cli/stdout@0.2.3 + wasi:io/streams@0.2.3 (no separate
    // package to push into the resolve).
    let _ = resolve
        .push_str("mighty-main.wit", &mighty_pkg_text)
        .map_err(|e| WasmError::Invalid(format!("wit p2 round-trip mighty: {e:#}")))?;
    if let Some(uw) = &opts.user_wit {
        let _ = resolve
            .push_str(&uw.source_label, &uw.text)
            .map_err(|e| WasmError::Invalid(format!("wit p2 round-trip user: {e:#}")))?;
    }

    // Surface a useful error early if the user-world name doesn't exist.
    // `select_world` only searches one package, so we iterate over
    // every package in the resolve and look for the world there.
    if let Some(name) = &user_world_name {
        let mut found = false;
        let pkg_ids: Vec<_> = resolve.packages.iter().map(|(id, _)| id).collect();
        for pkg_id in pkg_ids {
            if resolve.select_world(pkg_id, Some(name)).is_ok() {
                found = true;
                break;
            }
        }
        if !found {
            return Err(WasmError::Invalid(format!(
                "user world '{}' not found in any package",
                name
            )));
        }
    }

    Ok(WitDocument {
        text: user_body,
        package_id: format!("mighty:{}", pkg_id),
        world_name,
        target: WasmTarget::Wasi,
    })
}

/// Compile + wrap a program as a P2 Component Model component.
pub fn compile_program_to_bytes_p2(
    prog: &Program,
    opts: &Preview2Options,
) -> CompileResult<Vec<u8>> {
    let core = compile_program_to_bytes_with_preview(prog, WasmTarget::Wasi, EmitWasiPreview::P2)?;
    let doc = emit_wit_p2(prog, opts)?;
    wrap_p2(
        &core,
        &doc,
        opts.embed_adapter.as_ref(),
        opts.user_wit.as_ref(),
    )
}

/// Compile a program to `out` as a P2 component. Returns the WIT
/// document used (for callers that want to display it).
pub fn compile_program_to_file_p2(
    prog: &Program,
    opts: &Preview2Options,
    out: &std::path::Path,
) -> CompileResult<(Vec<u8>, WitDocument)> {
    let core = compile_program_to_bytes_with_preview(prog, WasmTarget::Wasi, EmitWasiPreview::P2)?;
    let doc = emit_wit_p2(prog, opts)?;
    let bytes = wrap_p2(
        &core,
        &doc,
        opts.embed_adapter.as_ref(),
        opts.user_wit.as_ref(),
    )?;
    std::fs::write(out, &bytes)
        .map_err(|e| WasmError::Io(format!("write {}: {}", out.display(), e)))?;
    Ok((bytes, doc))
}

/// Wrap a core module as a P2 component, doing the world lookup
/// across *every* package in the resolve (not just the synthesized
/// `mighty:<pkg>` one). This is the difference from
/// [`crate::component::wrap_as_component`], which assumes the world
/// lives in the document's primary package.
///
/// When `embed_adapter` is `Some`, the caller-supplied
/// `wasi_snapshot_preview1` adapter bytes are passed to
/// [`wit_component::ComponentEncoder::adapter`] so the core module's
/// P1-shaped imports are translated into versioned P2 calls at
/// instantiation. The adapter adds ~80 KB to the component (mostly
/// constant; wasmtime strips unused adapter exports during
/// `ComponentEncoder::encode`).
fn wrap_p2(
    core_module: &[u8],
    doc: &WitDocument,
    embed_adapter: Option<&AdapterEmbed>,
    user_wit: Option<&UserWit>,
) -> CompileResult<Vec<u8>> {
    // Re-derive the per-package text the way `emit_wit_p2` did so the
    // resolver sees each top-level `package` as a separate file (the
    // only way wit-parser permits cross-package references). The
    // canonical text we display in `doc.text` is *not* round-trip
    // parse-able as a single blob — that's a documented quirk of
    // the multi-package serialization.
    let mut resolve = wit_parser::Resolve::default();
    // Vendored P2 packages, one push_str per package.
    for (label, pkg_text) in split_nested_into_packages(VENDORED_WASI_P2_WIT, "vendored-p2") {
        let _ = resolve
            .push_str(&label, &pkg_text)
            .map_err(|e| WasmError::Invalid(format!("p2 wrap vendored: {e:#}")))?;
    }
    // v0.17: the unversioned `wasi:cli` shim package used to be
    // pushed here so wit-component could resolve the slice-8
    // emitter's literal `wasi:cli/log#log` import. With v0.17's
    // direct-lowering pass the core module never imports
    // `wasi:cli/log` anymore, so the shim package is gone.
    // Re-synthesize the mighty package so we know the package id for
    // its `select_world` call.
    let mighty_pkg_text = format!(
        "package {pkg_id};\n\n\
         world {world_name} {{\n\
         {imports}\
           export main: func();\n\
         }}\n",
        pkg_id = doc.package_id,
        world_name = if doc.world_name.is_empty() {
            "mighty-world".to_string()
        } else {
            doc.world_name.clone()
        },
        imports = synth_world_imports(),
    );
    let mighty_pkg_id = resolve
        .push_str("mighty-main.wit", &mighty_pkg_text)
        .map_err(|e| WasmError::Invalid(format!("p2 wrap mighty: {e:#}")))?;
    if let Some(uw) = user_wit {
        let _ = resolve
            .push_str(&uw.source_label, &uw.text)
            .map_err(|e| WasmError::Invalid(format!("p2 wrap user wit: {e:#}")))?;
    }

    // Find which package owns the world named `doc.world_name`. The
    // mighty package is the most likely candidate; if a user-WIT
    // overrode the world name, fall back to searching every package
    // in the resolve.
    let mut world_id = resolve
        .select_world(mighty_pkg_id, Some(&doc.world_name))
        .ok();
    if world_id.is_none() {
        // Lookup across every package — handles the user-supplied
        // world case (e.g. `[wit] world = "custom-world"` from a
        // demo:user-pkg package).
        let pkg_ids: Vec<_> = resolve.packages.iter().map(|(id, _)| id).collect();
        for pkg_id in pkg_ids {
            if let Ok(w) = resolve.select_world(pkg_id, Some(&doc.world_name)) {
                world_id = Some(w);
                break;
            }
        }
    }
    let world_id = world_id.ok_or_else(|| {
        WasmError::Invalid(format!(
            "p2 wrap: world '{}' not found in any package",
            doc.world_name
        ))
    })?;

    let mut module_bytes = core_module.to_vec();

    // The wasmtime adapter expects a *command*-shape core module
    // when [`AdapterKind::Command`] is in use, which means an
    // exported `_start: func()`. Mighty's slice-8 core module
    // exports `main` (not `_start`). Synthesize a `_start` export
    // that aliases `main` so the adapter's `wasi:cli/run.run`
    // re-export is satisfied.
    //
    // Only run this when the core module is missing `_start` —
    // if a future emitter path provides it directly we leave the
    // module untouched.
    if matches!(embed_adapter.map(|a| a.kind), Some(AdapterKind::Command))
        && !module_exports_func(&module_bytes, "_start")
    {
        module_bytes = alias_main_as_start(&module_bytes)?;
    }

    wit_component::embed_component_metadata(
        &mut module_bytes,
        &resolve,
        world_id,
        wit_component::StringEncoding::UTF8,
    )
    .map_err(|e| WasmError::Invalid(format!("p2 embed wit metadata: {e:#}")))?;

    let mut enc = wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module_bytes)
        .map_err(|e| WasmError::Invalid(format!("p2 component encoder module: {e:#}")))?;
    if let Some(adapter) = embed_adapter {
        enc = enc
            .adapter(adapter.kind.import_module_name(), &adapter.bytes)
            .map_err(|e| {
                WasmError::Invalid(format!(
                    "p2 adapter embed ({}): {e:#}",
                    adapter.kind.import_module_name()
                ))
            })?;
    }
    enc.encode()
        .map_err(|e| WasmError::Invalid(format!("p2 component encode: {e:#}")))
}

/// Return true iff `module_bytes` declares an exported function
/// named `name`.
fn module_exports_func(module_bytes: &[u8], name: &str) -> bool {
    use wasmparser::{ExternalKind, Parser, Payload};
    for payload in Parser::new(0).parse_all(module_bytes) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for ex in reader.into_iter().flatten() {
                if matches!(ex.kind, ExternalKind::Func) && ex.name == name {
                    return true;
                }
            }
        }
    }
    false
}

/// Find the func-index that's currently exported as `main`. Returns
/// `None` if no such export exists (the slice-8 emitter doesn't
/// always synthesize `main`).
fn find_main_export(module_bytes: &[u8]) -> Option<u32> {
    use wasmparser::{ExternalKind, Parser, Payload};
    for payload in Parser::new(0).parse_all(module_bytes) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for ex in reader.into_iter().flatten() {
                if matches!(ex.kind, ExternalKind::Func) && ex.name == "main" {
                    return Some(ex.index);
                }
            }
        }
    }
    None
}

/// Add an export `_start: func()` to `module_bytes`. The export
/// aliases the existing `main` export (the wasmtime command-adapter
/// invokes `_start`, which is what wasi-libc-built programs emit;
/// Mighty's slice-8 emitter still uses `main`).
///
/// Implementation: parse the existing module, copy every section
/// verbatim, and rewrite the export section to add `_start`. We
/// don't add a *new* function — the export simply points at the
/// same func index as `main`.
fn alias_main_as_start(module_bytes: &[u8]) -> CompileResult<Vec<u8>> {
    use wasm_encoder::{ExportKind as WExportKind, ExportSection, Module, RawSection};
    use wasmparser::{ExternalKind, Parser, Payload};

    let Some(main_idx) = find_main_export(module_bytes) else {
        // No `main` to alias from — return unchanged so the existing
        // diagnostics (from wit-component) surface.
        return Ok(module_bytes.to_vec());
    };

    // Walk payloads and rewrite the export section. wasm-encoder's
    // `RawSection` lets us splice unchanged sections back in
    // byte-for-byte.
    let mut new_module = Module::new();
    let mut handled_export = false;
    for payload in Parser::new(0).parse_all(module_bytes) {
        let payload =
            payload.map_err(|e| WasmError::Invalid(format!("alias_main_as_start parse: {e:#}")))?;
        match payload {
            Payload::Version { .. } => {}
            Payload::ExportSection(reader) => {
                let mut new_exports = ExportSection::new();
                for ex in reader {
                    let ex = ex.map_err(|e| WasmError::Invalid(format!("export iter: {e:#}")))?;
                    let kind = match ex.kind {
                        ExternalKind::Func | ExternalKind::FuncExact => WExportKind::Func,
                        ExternalKind::Table => WExportKind::Table,
                        ExternalKind::Memory => WExportKind::Memory,
                        ExternalKind::Global => WExportKind::Global,
                        ExternalKind::Tag => WExportKind::Tag,
                    };
                    new_exports.export(ex.name, kind, ex.index);
                }
                new_exports.export("_start", WExportKind::Func, main_idx);
                new_module.section(&new_exports);
                handled_export = true;
            }
            // Re-emit any other section verbatim. wasmparser's
            // `Payload::*::range()` gives us the original byte range.
            other => {
                if let Some((id, range)) = section_passthrough(&other) {
                    new_module.section(&RawSection {
                        id,
                        data: &module_bytes[range],
                    });
                }
            }
        }
    }
    // If the source module had no export section, append one with
    // just our synthetic `_start`. (Shouldn't happen for a Mighty-
    // compiled core module but the helper is defensive.)
    if !handled_export {
        let mut new_exports = ExportSection::new();
        new_exports.export("_start", WExportKind::Func, main_idx);
        new_module.section(&new_exports);
    }
    Ok(new_module.finish())
}

/// Return `Some((section_id, byte_range))` for the source-byte range
/// of any wasm payload we want to copy verbatim into the rewritten
/// module. Returns `None` for payloads that don't correspond to a
/// section we should pass through (e.g. `Payload::End`,
/// `Payload::Version`, the export section we're rewriting, etc.).
fn section_passthrough(payload: &wasmparser::Payload<'_>) -> Option<(u8, std::ops::Range<usize>)> {
    use wasmparser::Payload::*;
    match payload {
        TypeSection(s) => Some((1, s.range())),
        ImportSection(s) => Some((2, s.range())),
        FunctionSection(s) => Some((3, s.range())),
        TableSection(s) => Some((4, s.range())),
        MemorySection(s) => Some((5, s.range())),
        GlobalSection(s) => Some((6, s.range())),
        // ExportSection is rewritten above; do not pass through.
        ExportSection(_) => None,
        StartSection { range, .. } => Some((8, range.clone())),
        ElementSection(s) => Some((9, s.range())),
        CodeSectionStart { range, .. } => Some((10, range.clone())),
        DataSection(s) => Some((11, s.range())),
        DataCountSection { range, .. } => Some((12, range.clone())),
        CustomSection(s) => Some((0, s.range())),
        TagSection(s) => Some((13, s.range())),
        // Payloads with no on-the-wire bytes:
        Version { .. } | End(_) => None,
        // Component-model + GC payloads — we don't expect to see
        // these in a Mighty-emitted core module, but be defensive.
        _ => None,
    }
}

/// Build a *standalone* core Wasm module that imports a single P2
/// interface call described by `which` and re-exports it under
/// `_start`. Used by the v0.14 direct-lowering tests to demonstrate
/// the helper produces a wrapping-component whose imports reference
/// the versioned P2 interface verbatim (no `wasi_snapshot_preview1`
/// hop).
///
/// The module is intentionally trivial — one import, one `_start`
/// function that calls it and traps on the return — so the
/// `wit-component` machinery has nothing else to do besides wire the
/// versioned import through. Callers in real codegen splice the
/// import into the existing module-under-construction via the same
/// `(module_name, fn_name)` pair returned by
/// [`P2DirectImport::import_pair`].
///
/// This function only exists so that the test suite (and any future
/// integration test) can exercise the P2-direct path without having
/// to hand-roll a `wasm-encoder` module each time. **It is not on
/// the build path** — `compile_program_to_bytes_p2` still produces
/// the full Mighty core module from `Program`.
pub fn build_direct_p2_probe_module(which: P2DirectImport) -> Vec<u8> {
    use wasm_encoder::{
        CodeSection, EntityType, ExportKind, ExportSection, Function as WFn, FunctionSection,
        ImportSection, Instruction as I, MemoryType, Module, TypeSection, ValType,
    };

    let mut module = Module::new();

    // Pick the *exact* canonical-ABI core-Wasm signature
    // `wit-component` expects for this import, so its decode-world
    // pass accepts the probe module as a valid candidate component
    // input. The WIT shapes below are normative for WASI 0.2.3.
    //
    //   wasi:random/random#get-random-bytes(len: u64) -> list<u8>
    //     → canonical-ABI lift: `(param i64) (param i32)` — first
    //     arg is the length (u64), second is the return-area
    //     pointer at which to write `(ptr: i32, len: i32)`.
    //
    //   wasi:clocks/monotonic-clock#now() -> instant
    //     → `() -> i64` (instant is a u64 alias).
    //
    //   wasi:clocks/monotonic-clock#resolution() -> duration
    //     → `() -> i64` (duration is a u64 alias).
    //
    //   wasi:clocks/wall-clock#now() -> datetime
    //     → `(param i32)` — the i32 is the return-area pointer
    //     for the `record datetime { seconds: u64, nanoseconds: u32 }`.
    let (params, results): (&[ValType], &[ValType]) = match which {
        P2DirectImport::RandomBytes => (&[ValType::I64, ValType::I32], &[]),
        P2DirectImport::MonotonicNow | P2DirectImport::MonotonicResolution => {
            (&[], &[ValType::I64])
        }
        P2DirectImport::WallClockNow => (&[ValType::I32], &[]),
        // v0.16 — filesystem.
        P2DirectImport::FsOpenAt => (
            &[
                ValType::I32, // self
                ValType::I32, // path-flags
                ValType::I32, // path-ptr
                ValType::I32, // path-len
                ValType::I32, // open-flags
                ValType::I32, // descriptor-flags
                ValType::I32, // ret-area
            ],
            &[],
        ),
        P2DirectImport::FsReadViaStream | P2DirectImport::FsWriteViaStream => {
            (&[ValType::I32, ValType::I64, ValType::I32], &[])
        }
        P2DirectImport::FsStat => (&[ValType::I32, ValType::I32], &[]),
        P2DirectImport::FsClose => (&[ValType::I32], &[]),
        // v0.16 — http.
        P2DirectImport::HttpNewRequest => (&[ValType::I32], &[ValType::I32]),
        P2DirectImport::HttpHandleRequest => (
            &[ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            &[],
        ),
        P2DirectImport::HttpResponseStatus => (&[ValType::I32], &[ValType::I32]),
        P2DirectImport::HttpResponseBody => (&[ValType::I32, ValType::I32], &[]),
        // v0.17 — log() direct lowerings.
        //   get-stdout: () -> output-stream   → `() -> i32`.
        //   blocking-write-and-flush: (self, ptr, len, ret_area) -> ()
        //   [resource-drop]output-stream: (self) -> ()
        P2DirectImport::LogStdoutGet => (&[], &[ValType::I32]),
        P2DirectImport::LogStreamWrite => (
            &[ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            &[],
        ),
        P2DirectImport::LogStreamDrop => (&[ValType::I32], &[]),
    };

    let mut types = TypeSection::new();
    types
        .ty()
        .function(params.iter().copied(), results.iter().copied());
    types.ty().function([], []);
    module.section(&types);

    let (mod_name, fn_name) = which.import_pair();
    let mut imports = ImportSection::new();
    imports.import(mod_name, fn_name, EntityType::Function(0));
    module.section(&imports);

    let mut funcs = FunctionSection::new();
    funcs.function(1);
    module.section(&funcs);

    // Minimal `(memory 1)` so wit-component's canonical-ABI lifting
    // has somewhere to land its returned-list payload.
    let mut memory = wasm_encoder::MemorySection::new();
    memory.memory(MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    module.section(&memory);

    let mut exports = ExportSection::new();
    exports.export("memory", ExportKind::Memory, 0);
    // `_start` is function index 1 (after the 1 imported fn).
    exports.export("_start", ExportKind::Func, 1);
    // wit-component's canonical-ABI lifter needs a `cabi_realloc`
    // export when an import returns an owned `list<u8>` /
    // `datetime`. Provide a no-op realloc that returns its `old`
    // pointer unchanged — the probe never *calls* the import, so
    // realloc is never invoked; the export just has to exist.
    exports.export("cabi_realloc", ExportKind::Func, 2);
    module.section(&exports);

    let mut code = CodeSection::new();
    let mut start = WFn::new([]);
    start.instruction(&I::End);
    code.function(&start);
    // `cabi_realloc(old: i32, old_size: i32, align: i32, new_size: i32) -> i32`
    // Simplest body: return 0 (the spec allows returning the
    // pointer the host wants to interpret as the new heap location;
    // since the probe never invokes the import, the return is dead).
    let realloc_ty = 2;
    let _ = realloc_ty; // for clarity / future expansion
    let mut realloc = WFn::new([]);
    realloc.instruction(&I::I32Const(0));
    realloc.instruction(&I::End);
    code.function(&realloc);
    module.section(&code);

    // Update the type/funcs sections to reflect the realloc.
    // wasm-encoder builds sections in calls above; we re-build them
    // with the realloc included to keep indices consistent.
    let mut module2 = Module::new();
    let mut types2 = TypeSection::new();
    types2
        .ty()
        .function(params.iter().copied(), results.iter().copied());
    types2.ty().function([], []);
    types2.ty().function(
        [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
        [ValType::I32],
    );
    module2.section(&types2);
    let mut imports2 = ImportSection::new();
    imports2.import(mod_name, fn_name, EntityType::Function(0));
    module2.section(&imports2);
    let mut funcs2 = FunctionSection::new();
    funcs2.function(1); // _start type
    funcs2.function(2); // cabi_realloc type
    module2.section(&funcs2);
    let mut mem2 = wasm_encoder::MemorySection::new();
    mem2.memory(MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    module2.section(&mem2);
    let mut exports2 = ExportSection::new();
    exports2.export("memory", ExportKind::Memory, 0);
    exports2.export("_start", ExportKind::Func, 1);
    exports2.export("cabi_realloc", ExportKind::Func, 2);
    module2.section(&exports2);
    let mut code2 = CodeSection::new();
    let mut start_body = WFn::new([]);
    start_body.instruction(&I::End);
    code2.function(&start_body);
    let mut realloc_body = WFn::new([]);
    realloc_body.instruction(&I::I32Const(0));
    realloc_body.instruction(&I::End);
    code2.function(&realloc_body);
    module2.section(&code2);

    // Throw away the partially-built `module`; module2 is the real
    // returned bytes.
    let _ = module;
    module2.finish()
}

/// Body lines for the synthesized world's import section. Lifted out
/// of `emit_wit_p2` so both that function and `wrap_p2` can share the
/// same import list when assembling per-package WIT files.
fn synth_world_imports() -> String {
    let lines = [
        "  import wasi:cli/environment@0.2.3;",
        "  import wasi:cli/exit@0.2.3;",
        "  import wasi:cli/stdin@0.2.3;",
        "  import wasi:cli/stdout@0.2.3;",
        "  import wasi:cli/stderr@0.2.3;",
        "  import wasi:io/error@0.2.3;",
        "  import wasi:io/poll@0.2.3;",
        "  import wasi:io/streams@0.2.3;",
        "  import wasi:clocks/monotonic-clock@0.2.3;",
        "  import wasi:clocks/wall-clock@0.2.3;",
        "  import wasi:random/random@0.2.3;",
        "  import wasi:filesystem/preopens@0.2.3;",
        "  import wasi:filesystem/types@0.2.3;",
        "  import wasi:http/types@0.2.3;",
        "  import wasi:http/outgoing-handler@0.2.3;",
        // v0.17 — the v0.14 `import wasi:cli/log;` shim line was
        // removed because the emitter now lowers log() directly to
        // `wasi:cli/stdout@0.2.3` (already imported above) +
        // `wasi:io/streams@0.2.3#output-stream.blocking-write-and-flush`
        // (the io/streams package is already imported above too).
    ];
    let mut s = String::new();
    for l in lines {
        s.push_str(l);
        s.push('\n');
    }
    s
}

/// Topological order for the vendored WASI Preview 2 packages.
/// `Resolve::push_str` rejects forward references between top-level
/// files — every `use wasi:io/...` requires that `wasi:io` already
/// be in the resolve. The ordering below matches the WASI 0.2.3
/// package dependency DAG:
///
/// ```text
///   wasi:io      ── no deps
///   wasi:clocks  → wasi:io
///   wasi:random  → no deps
///   wasi:sockets → wasi:io
///   wasi:filesystem → wasi:io, wasi:clocks
///   wasi:cli     → all of the above
///   wasi:http    → wasi:io, wasi:clocks, wasi:cli
/// ```
const VENDORED_P2_PKG_ORDER: &[&str] = &[
    "wasi:io@0.2.3",
    "wasi:clocks@0.2.3",
    "wasi:random@0.2.3",
    "wasi:sockets@0.2.3",
    "wasi:filesystem@0.2.3",
    "wasi:cli@0.2.3",
    "wasi:http@0.2.3",
];

/// Split a multi-package text (nested `package X:Y { ... }` blocks
/// at the top level) into individual top-level files, one per
/// package. The returned strings carry a `package X:Y;` declaration
/// (no braces) followed by the original block body, ready to feed
/// directly to `wit_parser::Resolve::push_str`.
///
/// The order of returned chunks matches the
/// [`VENDORED_P2_PKG_ORDER`] DAG so callers can push them straight
/// into a resolve without worrying about forward-reference errors.
///
/// `label_prefix` is used to disambiguate the filename labels we
/// hand to `push_str` (these only appear in parser diagnostics).
fn split_nested_into_packages(text: &str, label_prefix: &str) -> Vec<(String, String)> {
    // Triple is (filename label, package name, chunk text). The package
    // name is used only to topologically order the chunks before they're
    // handed back to the caller as `(label, chunk)`.
    let mut out: Vec<(String, String, String)> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    let n = bytes.len();
    while i < n {
        // Skip whitespace + comment lines until we see a `package`
        // keyword followed by an open brace.
        // A simple approach: scan for the substring `package ` at
        // start of a logical line, then look for the matching `{`.
        let rest = &text[i..];
        // Skip leading whitespace + comments
        let start = match find_next_package_block(rest) {
            Some(p) => p + i,
            None => break,
        };
        // Parse `package <name> {` -- extract the name and the brace
        // position.
        let pkg_start = &text[start..];
        let Some((pkg_name, brace_open)) = parse_package_header(pkg_start) else {
            i = start + 1;
            continue;
        };
        // Find matching close brace.
        let body_start = start + brace_open + 1;
        let mut depth: u32 = 1;
        let mut j = body_start;
        while j < n && depth > 0 {
            match bytes[j] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        if depth != 0 {
            // unbalanced — bail. The error will surface when we feed
            // the truncated chunk to `push_str` below.
            break;
        }
        let body = &text[body_start..(j - 1)];
        let label = format!(
            "{label_prefix}-{}.wit",
            pkg_name.replace(':', "-").replace('@', "_")
        );
        let chunk = format!("package {pkg_name};\n{body}\n");
        out.push((label, pkg_name, chunk));
        i = j;
    }
    // Topologically reorder using `VENDORED_P2_PKG_ORDER`. Any chunk
    // not in the ordering list is appended at the end (preserves the
    // text-order for new packages someone forgot to add to the
    // ordering const).
    let mut ordered: Vec<(String, String)> = Vec::with_capacity(out.len());
    for target in VENDORED_P2_PKG_ORDER {
        if let Some(pos) = out.iter().position(|(_, name, _)| name == target) {
            let (label, _name, chunk) = out.swap_remove(pos);
            ordered.push((label, chunk));
        }
    }
    for (label, _, chunk) in out {
        ordered.push((label, chunk));
    }
    ordered
}

/// Return the byte offset of the next `package` keyword that begins a
/// top-level nested block (look for `package <name> {`). Returns `None`
/// when no such block is found in `text`.
fn find_next_package_block(text: &str) -> Option<usize> {
    let mut search_from = 0;
    while let Some(pos) = text[search_from..].find("package ") {
        let absolute = search_from + pos;
        // Only accept matches that look like the start of a top-level
        // decl: preceded by start-of-string or a newline (and only
        // whitespace between the newline and the keyword).
        let prefix_ok = if absolute == 0 {
            true
        } else {
            let prev = &text[..absolute];
            // Walk backward over the same line — accept only spaces/tabs.
            let line_start = prev.rfind('\n').map(|p| p + 1).unwrap_or(0);
            text[line_start..absolute]
                .chars()
                .all(|c| c == ' ' || c == '\t')
        };
        // Also require that this is a *block* form, not a `package X;`
        // top-level decl. Look ahead for the next non-comment token to
        // see if it's `{`. Cheap proxy: scan forward to the next `{`
        // or `;` and check which comes first.
        let after = &text[absolute..];
        let semi = after.find(';');
        let brace = after.find('{');
        let is_block = match (semi, brace) {
            (Some(s), Some(b)) => b < s,
            (None, Some(_)) => true,
            _ => false,
        };
        if prefix_ok && is_block {
            return Some(absolute);
        }
        search_from = absolute + "package ".len();
    }
    None
}

/// Given text starting with `package <name> {`, return the package
/// name (without braces) and the byte offset of the opening `{`.
fn parse_package_header(text: &str) -> Option<(String, usize)> {
    let after = text.strip_prefix("package ")?;
    let brace = after.find('{')?;
    let name = after[..brace].trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some((name, "package ".len() + brace))
}

// `wrap_user_wit_as_nested` (v0.13) used to fold user `.wit` text into
// nested-package form so it could be concatenated into one big blob
// passed to `Resolve::push_str`. The v0.14 architecture pushes each
// top-level package separately, so the helper is no longer needed —
// user WIT is fed verbatim to the resolver.

/// Sanitize a free-form package name into a kebab-case WIT id.
///
/// Mirrors `wit::sanitize_pkg_id` but is duplicated here to avoid
/// reaching into a private helper. Cheap enough that we don't bother
/// hoisting it.
fn sanitize_pkg_id(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if c == '-' || c == '_' {
            s.push('-');
        }
    }
    if s.is_empty() || !s.chars().next().unwrap().is_ascii_alphabetic() {
        s.insert(0, 'p');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use mty_hir::SourceSpan;
    use mty_ir::ir::{
        Block, BlockId, Const, Function, IrFnId, IrTy, LocalDecl, LocalSource, Operand, Program,
        Term,
    };

    fn empty_main() -> Program {
        let mut p = Program::default();
        p.fns.push(Function {
            id: IrFnId(0),
            name: "main".into(),
            params: vec![],
            locals: vec![LocalDecl {
                name: "_0".into(),
                ty: IrTy::Unit,
                mutable: false,
                source: LocalSource::Return,
            }],
            blocks: vec![Block {
                id: BlockId(0),
                stmts: vec![],
                terminator: Term::Return(Operand::Const(Const::Unit)),
            }],
            entry: BlockId(0),
            ret_ty: IrTy::Unit,
            effects: vec![],
            hir_fn: None,
            span: SourceSpan { start: 0, end: 0 },
        });
        p
    }

    #[test]
    fn p2_wit_round_trips() {
        let opts = Preview2Options::new("hello");
        let doc = emit_wit_p2(&empty_main(), &opts).expect("emit p2 wit");
        assert!(doc.text.contains("wasi:io/streams@0.2.3"));
        assert!(doc.text.contains("wasi:cli/stdout@0.2.3"));
        assert!(doc.text.contains("wasi:http/outgoing-handler@0.2.3"));
        // Note: `doc.text` is a multi-package display blob — under
        // the v0.14 architecture it does NOT round-trip through a
        // single `Resolve::push_str` call (each top-level package
        // must be pushed separately). The end-to-end validation that
        // the merge succeeded is `emit_wit_p2`'s internal round-trip
        // (run before `Ok(doc)` returns above), which has already
        // succeeded by the time we get here.
    }

    #[test]
    fn p2_component_wraps() {
        let opts = Preview2Options::new("hello");
        let bytes = compile_program_to_bytes_p2(&empty_main(), &opts).expect("compile p2");
        assert!(crate::component::is_component(&bytes));
    }
}
