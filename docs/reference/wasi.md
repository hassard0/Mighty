# WASI compatibility

Mighty targets the WebAssembly Component Model on every `mty build
--target wasm32-*` invocation. Through v0.12 the only host shape was
**WASI Preview 1** (P1) — the imports the slice-8 emitter wires
(`wasi:cli/log#log`) match the P1 snapshot interface.

v0.13 introduced an opt-in **WASI Preview 2** (P2) backend driven by
the `--wasi=p2` flag. **v0.15 flips the default**: passing
`--target wasm32-wasi` without `--wasi=...` now produces a P2
component. `--wasi=p1` keeps the legacy import shape for back-compat.
This page describes both modes, the compatibility matrix, and how to
consume user-authored `.wit` files.

## TL;DR

```bash
# Default since v0.15 — P2 component with versioned wasi:*@0.2.3
# imports + the vendored P1→P2 adapter for the surfaces that still
# route through it.
mty build hello.mty --target wasm32-wasi

# Explicit opt-in to P2 (identical to the default since v0.15;
# retained for explicit-intent build scripts).
mty build hello.mty --target wasm32-wasi --wasi=p2

# Back-compat — keep the v0.2..v0.14 P1 import shape. Useful for
# downstream tooling that hasn't moved to the Component Model yet.
mty build hello.mty --target wasm32-wasi --wasi=p1

# P2 + user-supplied WIT (worlds defined in mighty.toml `[wit]`).
mty build hello.mty --target wasm32-wasi --world my-world
```

## Compatibility matrix

| Mighty surface | P1 (`--wasi=p1`) | P2 (default since v0.15) |
|----------------|--------------|------------------|
| `log()` / `print()` | imports `wasi:cli/log` (back-compat shim) | **direct P2 import** `wasi:cli/stdout@0.2.3#get-stdout` + `wasi:io/streams@0.2.3#[method]output-stream.blocking-write-and-flush` + `[resource-drop]output-stream` (wired into emitter v0.17, this slice) |
| `std.fs.open()` / `read_file()` / `write_file()` / `stat()` / `close()` | P1 syscall | **direct P2 import** `wasi:filesystem/types@0.2.3` resource methods (wired into emitter v0.16) |
| `std.http.get()` / `post()` / `send()` | P1 syscall | **direct P2 import** `wasi:http/types@0.2.3` constructor + `wasi:http/outgoing-handler@0.2.3#handle` (wired into emitter v0.16) |
| `std.time.now()` / `monotonic_now()` / `resolution()` | P1 syscall | **direct P2 import** `wasi:clocks/{wall-clock,monotonic-clock}@0.2.3` (wired into emitter v0.15) |
| `std.random.bytes()` / `u64()` | P1 syscall | **direct P2 import** `wasi:random/random@0.2.3` (wired into emitter v0.15) |
| `wasi_snapshot_preview1` adapter | n/a | **opt-in** (default `None` since v0.17). Set via `Preview2Options::with_adapter(Some(AdapterEmbed::new(AdapterKind::Command, bytes)))` for builds that link wasi-libc-built C code. v0.19 stopped vendoring the adapter binaries in-tree — callers supply their own bytes (download `wasi_snapshot_preview1.command.wasm` from the matching wasmtime release; v32.0.0 targets WASI 0.2.3). |
| User-WIT `[wit]` section | ignored | merged into world |
| `--world <name>` | ignored | picks world from user WIT |

Two transport modes coexist in v0.14:

- **direct P2** — the core module imports the versioned
  `wasi:*@0.2.3` interface verbatim. The component-level WIT
  contract references the same interface; a strict P2 host wires
  it directly.
- **adapter-routed** — the core module imports the legacy
  `wasi_snapshot_preview1` syscall (the wasi-libc convention).
  At wrap time, `wit-component::ComponentEncoder::adapter(...)`
  embeds a caller-supplied adapter (~54 KB of Wasm per kind)
  which translates each P1 call into the matching versioned P2
  interface call at instantiation. v0.13–v0.18 vendored the
  wasmtime v32.0.0 adapter under `crates/mty-codegen-wasm/adapter/`;
  v0.19 stopped vendoring (the default path doesn't import P1
  anymore, so paying the 150 KB cost on every `cargo build` was
  pure waste) — callers now download the bytes from the matching
  wasmtime release and pass them via `AdapterEmbed`. From the
  host's perspective the component imports the same
  `wasi:*@0.2.3` interfaces either way.

v0.16 flipped `std.fs` + `std.http` to the direct path; v0.17
finishes the job by flipping `log()` to direct lowering AND
inverting the adapter's default to opt-in. Programs that touch
only `std.random`, `std.time`, `std.fs`, `std.http`, and `log()`
now ship adapter-free by default — the `wasi_snapshot_preview1`
adapter is opted back in only when a build links a wasi-libc-built
C crate or otherwise reaches for the legacy P1 syscall shape.

Component-size impact: a Mighty `log()`-only command component now
ships ≥ ~50 KB smaller than the v0.16 baseline (the adapter binary
is ~54 KB; wit-component's tree-shaking previously trimmed it
toward ~12 KB on adapter-heavy programs, but the floor remains
non-trivial). The `tests/preview2_log.rs::log_program_compiles_adapter_free_by_default`
smoke test pins that an adapter-free build validates end-to-end;
the v0.18-era `log_program_no_adapter_runs_smaller` size-comparison
test was retired in v0.19 along with the vendored bytes (driving the
adapter path now requires the caller to supply real wasmtime
adapter bytes, which are no longer guaranteed to be present in
CI).

## How the P2 backend works

A P2 build (the v0.15 default) does five things differently from the
explicit `--wasi=p1` legacy path:

1. The generated WIT document imports versioned P2 interfaces:
   `wasi:cli@0.2.3`, `wasi:io@0.2.3`, `wasi:clocks@0.2.3`,
   `wasi:filesystem@0.2.3`, `wasi:http@0.2.3`, `wasi:random@0.2.3`.
2. The component's package id becomes `mty:<pkg>` (renamed from the
   pre-v0.36 `stardust:<pkg>` spelling; see
   `crates/mty-codegen-wasm/src/wit.rs::PKG_NAMESPACE`).
3. (v0.13–v0.16) An unversioned `wasi:cli/log` shim used to be
   declared here so the core module's `wasi:cli/log#log` import
   resolved through `wit-component::ComponentEncoder` without
   modification. **v0.17 dropped the shim entirely** — `log()` now
   lowers directly to a three-call canonical-ABI sequence on top
   of `wasi:cli/stdout@0.2.3` + `wasi:io/streams@0.2.3`.
4. The `wasi_snapshot_preview1` adapter is *opt-in* since v0.17.
   v0.19 stopped vendoring the bytes — pass
   `Preview2Options::with_adapter(Some(AdapterEmbed::new(AdapterKind::Command, bytes)))`
   to embed it when a build links wasi-libc-built C code, where
   `bytes` is the matching wasmtime release's
   `wasi_snapshot_preview1.command.wasm` (~54 KB). It translates
   any P1-shaped syscall the core
   module makes into the matching versioned `wasi:*@0.2.3`
   interface call at instantiation.
5. The core module's `main` export is aliased as `_start` *only*
   when the adapter is opted in (the wasmtime command-adapter
   expects the wasi-libc / clang `_start` entry-point name). Pure
   Mighty programs that never touch P1 syscalls keep `main` as the
   single entry point.

## Authoring a `.wit` file

The Component Model expects a WIT package that declares **types**,
**interfaces**, and **worlds**. A minimal user package looks like
this:

```wit
// wit/greeter.wit
package demo:greeter;

interface api {
  greet: func(name: string) -> string;
}

world greeter-world {
  import wasi:cli/stdout@0.2.3;
  export api;
}
```

To wire it into a Mighty build, add a `[wit]` section to
`mighty.toml`:

```toml
[wit]
# Optional — picks the world by name when the user package defines
# more than one. Same effect as `--world greeter-world` on the CLI.
world = "greeter-world"

# Relative paths to .wit files. Order doesn't matter; the
# Wasm-codegen merges them all into one resolve.
files = ["wit/greeter.wit"]
```

Then build with `--wasi=p2`:

```bash
mty build src/main.mty --target wasm32-wasi --wasi=p2
```

The emitted component will:

- export the `api` interface from `greeter-world`,
- import everything Mighty's synthesized world imports,
- plus everything the user world declares (`wasi:cli/stdout` in this
  example).

### What gets merged

The user WIT is concatenated *after* Mighty's synthesized package and
the vendored P2 stubs. Conflicts (two `package X:Y;` declarations
with the same id, two worlds with the same name) surface as parse
errors from `wit_parser::Resolve`.

### User world inheritance

A user-defined world **replaces** the synthesized `<pkg>-world` — it
does not extend it. So the user world must re-declare every host
capability the core module relies on. For v0.17 that means:

- `import wasi:cli/stdout@0.2.3;` and `import wasi:io/streams@0.2.3;`
  (the v0.17 emitter's `log()` direct-lowering needs both — the
  v0.13 `wasi:cli/log` shim is gone).
- Any other P2 imports your program uses directly (`wasi:random/random@0.2.3`,
  `wasi:clocks/monotonic-clock@0.2.3`, …).

User worlds may also declare:

- **Custom imports**: new interfaces the host provides. End-to-end
  in v0.13.
- **Custom exports**: limited to `export main: func();` for v0.13.
  Richer exports require the core module to provide matching
  functions, which the slice-8 emitter doesn't synthesize yet.

The `wit/example/hello-world.wit` file shipped in the repo is the
canonical reference shape.

### Multiple worlds

When the user package declares more than one world, the build either:

- Picks the one named in `[wit] world = "..."`,
- or the one given as `--world <name>` (overrides the manifest), or
- fails with an "ambiguous world" error.

## Which `std.*` modules lower to P2?

v0.17 status:

- **direct P2 lowering** (wired into the core-module emitter via
  `mty_codegen_wasm::P2DirectImport`):
  - `std.time.now()`, `std.time.monotonic_now()`,
    `std.time.resolution()` → `wasi:clocks/*@0.2.3` (v0.15).
  - `std.random.bytes()`, `std.random.u64()` →
    `wasi:random/random@0.2.3` (v0.15).
  - `std.fs.open()` / `read_file()` / `write_file()` / `stat()` /
    `close()` → `wasi:filesystem/types@0.2.3.descriptor.*` and
    `[resource-drop]descriptor` (v0.16).
  - `std.http.get()` / `post()` / `send()` →
    `wasi:http/types@0.2.3.[constructor]outgoing-request` +
    `wasi:http/outgoing-handler@0.2.3#handle` (v0.16).
  - `log()` / `print()` →
    `wasi:cli/stdout@0.2.3#get-stdout` +
    `wasi:io/streams@0.2.3#[method]output-stream.blocking-write-and-flush`
    + `[resource-drop]output-stream` (v0.17, this slice — see
    [`emit_log_call_sequence`] for the helper).

  Each of these splices the versioned P2 interface into the
  core module's import section verbatim — no adapter hop needed.
  The v0.17 lowering for `log()` uses the blocking-write variant
  so the slice-8 `log()` builtin's fire-and-forget semantics
  carry over to P2 without threading a `wasi:io/poll.pollable`
  through Mighty's surface.

Components that touch ONLY the surfaces above ship adapter-free
since v0.17 — `Preview2Options::default().embed_adapter` is now
`None`. To opt the adapter back in (when linking wasi-libc-built
C code, for instance) pass
`with_adapter(Some(AdapterEmbed::new(AdapterKind::Command, bytes)))`
when constructing the options. v0.19 stopped vendoring the
adapter binaries in-tree, so `bytes` is caller-supplied —
download the matching wasmtime release's
`wasi_snapshot_preview1.command.wasm` and pass it in.

### v0.16 lifecycle notes (fs + http)

The v0.16 fs + http lowerings are intentionally **blocking-style**
and conservative on resource-handle lifecycle:

- `std.fs.read_file(path)` lowers to a single
  `descriptor.read-via-stream` call with a placeholder
  `descriptor=0` handle. The full open → read → close scaffold
  (which would also splice the
  `[resource-drop]descriptor` import per call) is a v0.17
  follow-up tracked against the SIR layer's preopen-handle
  lifting work. What v0.16 PINS is that the versioned import
  lands in the import section (so a strict P2 host wires it
  through) and the component validates.
- `std.http.send(req)` lowers to `outgoing-handler.handle` with
  placeholder argument handles. The full
  `subscribe()` / `get()` poll loop for
  `future-incoming-response` is the v0.17 follow-up.

## Versioning

Mighty v0.15 targets **WASI 0.2.3**. The exact version string is
exposed as `mty_codegen_wasm::WASI_P2_VERSION`. The vendored P2 WIT
text lives at `crates/mty-codegen-wasm/wit/wasi-p2/wasi-p2.wit`
(the full upstream wasi-cli + wasi-http surface, concatenated).

The P1→P2 adapter binaries (`wasi_snapshot_preview1.{command,
reactor,proxy}.wasm`) used to be vendored alongside the WIT at
`crates/mty-codegen-wasm/adapter/`. **v0.19 deleted them** — the
default Mighty build does not import `wasi_snapshot_preview1`
anywhere, so the ~150 KB cost (3 × ~50 KB) was pure dead weight
on every consumer's `cargo build`. Callers that still need the
adapter download the bytes from the matching wasmtime release
(WASI 0.2.3 first stabilized in wasmtime v32.0.0) and pass them
via `AdapterEmbed`.

## Roadmap

- v0.14 (shipped): vendored P1→P2 adapter embedded by default;
  `std.random` + `std.time` direct-import helpers landed as
  constants; emitter still routed those calls through the
  Unsupported fallback.
- v0.15 (shipped): direct-import dispatch wired through emit.rs for
  `std.random.bytes` + `std.time.{now,monotonic_now,resolution}`;
  `--wasi=p2` is now the default for `wasm32-wasi`; `--wasi=p1`
  remains supported for back-compat.
- v0.16 (shipped): direct lowering for
  `std.fs.{open,read_file,write_file,stat,close}` and
  `std.http.{get,post,send}`; canonical-ABI helpers
  (`emit_resource_drop_call` / per-variant signatures);
  pre-decl pass in the emitter to keep function indices stable
  when the lazy import-declaration adds a new import mid-body.
- v0.17 (shipped, this slice): direct lowering for `log()` (drops
  the `wasi:cli/log` shim and replaces it with a three-call
  canonical-ABI sequence on top of `wasi:cli/stdout@0.2.3` +
  `wasi:io/streams@0.2.3` via the new `emit_log_call_sequence`
  helper); adapter default flipped from
  `Some(AdapterKind::Command)` to `None` — opt-in via
  `with_adapter(Some(...))` for builds that link wasi-libc-built
  C code.
- v0.18: full resource-handle lifecycle for `std.fs` (open +
  close scaffold around read/write/stat); full streaming layer
  for `std.http` (subscribe + poll loop on
  `future-incoming-response`).
- v0.19 (shipped): the vendored
  `wasi_snapshot_preview1.{command,reactor,proxy}.wasm`
  binaries are removed from `crates/mty-codegen-wasm/adapter/`.
  Every Mighty-emitted program is adapter-free; callers that
  link wasi-libc-built C code now download the matching
  wasmtime release's adapter and pass the bytes via
  `AdapterEmbed`. Closes the last v0.17/v0.18 carryover.
- v1.0 RC4: P1 becomes a tier-2 target (still emitted on request but
  no longer the default; the documentation tree assumes P2).

See `dev/history/notes/WASI_P2_V0_13_NOTES.md` for the v0.13
plan + open decisions,
`dev/history/notes/WASI_P2_LOWERINGS_V0_14_NOTES.md` for the v0.14
follow-up, `dev/history/notes/WASI_P2_FINISH_V0_15_NOTES.md` for
the v0.15 default-flip + emitter-wiring rationale,
`dev/history/notes/WASI_P2_FS_HTTP_V0_16_NOTES.md` for the v0.16
fs + http direct-lowering rationale, and
`dev/history/notes/WASI_P2_LOG_V0_17_NOTES.md` for the v0.17
`log()` direct-lowering + adapter-opt-out rationale.
