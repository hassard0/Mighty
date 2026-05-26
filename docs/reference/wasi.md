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
| `log()` / `print()` | imports `wasi:cli/log` | imports `wasi:cli/log` (unversioned shim, **deprecated for v0.17** — will route to `wasi:cli/stdout@0.2.3` + `wasi:io/streams@0.2.3#output-stream.blocking-write-and-flush`) |
| `std.fs.open()` / `read_file()` / `write_file()` / `stat()` / `close()` | P1 syscall | **direct P2 import** `wasi:filesystem/types@0.2.3` resource methods (wired into emitter v0.16) |
| `std.http.get()` / `post()` / `send()` | P1 syscall | **direct P2 import** `wasi:http/types@0.2.3` constructor + `wasi:http/outgoing-handler@0.2.3#handle` (wired into emitter v0.16) |
| `std.time.now()` / `monotonic_now()` / `resolution()` | P1 syscall | **direct P2 import** `wasi:clocks/{wall-clock,monotonic-clock}@0.2.3` (wired into emitter v0.15) |
| `std.random.bytes()` / `u64()` | P1 syscall | **direct P2 import** `wasi:random/random@0.2.3` (wired into emitter v0.15) |
| `wasi_snapshot_preview1` adapter | n/a | **embedded** (~54 KB) — wasmtime v32.0.0 command shape; still needed for `log()` until v0.17, but most stdlib calls now bypass it |
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
  embeds the vendored adapter (~54 KB of Wasm shipped under
  `crates/mty-codegen-wasm/adapter/`) which translates each P1
  call into the matching versioned P2 interface call at
  instantiation. From the host's perspective the component
  imports the same `wasi:*@0.2.3` interfaces either way.

v0.16 flips `std.fs` + `std.http` to the direct path, leaving
only `log()` and a handful of minor utilities (e.g. `exit`,
`environment.get`) still adapter-routed. The full migration to a
pure-direct P2 surface — at which point the adapter can be opted
out via `Preview2Options::with_adapter(None)` for smaller
components — is the v0.17 follow-up. v0.15 wired direct
lowerings for `std.random.bytes` and
`std.time.{now,monotonic_now,resolution}`; v0.16 extends that to
`std.fs.{open,read_file,write_file,stat,close}` and
`std.http.{get,post,send}`. Building any program that uses those
calls under `--wasi=p2` (the default) splices the versioned
imports directly into the core module's import section.

Component-size impact: `std.fs` + `std.http` programs no longer
pull the `wasi_snapshot_preview1#fd_*` / `sock_*` adapter trampolines
into the linked component. The adapter Wasm itself is still
embedded (for `log()`), but `wit-component`'s tree-shaking now
strips the unused fs + http translation paths, reducing the
adapter contribution from ~54 KB toward ~12 KB on
fs+http-heavy programs.

## How the P2 backend works

A P2 build (the v0.15 default) does five things differently from the
explicit `--wasi=p1` legacy path:

1. The generated WIT document imports versioned P2 interfaces:
   `wasi:cli@0.2.3`, `wasi:io@0.2.3`, `wasi:clocks@0.2.3`,
   `wasi:filesystem@0.2.3`, `wasi:http@0.2.3`, `wasi:random@0.2.3`.
2. The component's package id becomes `mighty:<pkg>` (instead of
   `stardust:<pkg>`).
3. The unversioned `wasi:cli/log` shim is declared so the core
   module's existing `wasi:cli/log#log` import keeps wiring through
   `wit-component::ComponentEncoder` without modification.
4. The vendored `wasi_snapshot_preview1` adapter (wasmtime v32.0.0
   build) is embedded into the component via
   `ComponentEncoder::adapter`. The adapter is ~54 KB of Wasm and
   translates any P1-shaped syscall the core module makes into the
   matching versioned `wasi:*@0.2.3` interface call at
   instantiation.
5. The core module's `main` export is aliased as `_start` so the
   wasmtime command-adapter (which expects the wasi-libc / clang
   `_start` entry-point name) wires up cleanly.

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
capability the core module relies on. For v0.13 that means:

- `import wasi:cli/log;` (the slice-8 emitter still wires `log()`
  through this interface; v0.14 will move it to `wasi:cli/stdout`)
- Any P2 imports your program uses directly (e.g.
  `wasi:cli/stdout@0.2.3`)

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

v0.16 status:

- **direct P2 lowering** (wired into the core-module emitter via
  `mty_codegen_wasm::P2DirectImport`):
  - `std.time.now()`, `std.time.monotonic_now()`,
    `std.time.resolution()` → `wasi:clocks/*@0.2.3` (v0.15).
  - `std.random.bytes()`, `std.random.u64()` →
    `wasi:random/random@0.2.3` (v0.15).
  - `std.fs.open()` / `read_file()` / `write_file()` / `stat()` /
    `close()` → `wasi:filesystem/types@0.2.3.descriptor.*` and
    `[resource-drop]descriptor` (v0.16, this slice).
  - `std.http.get()` / `post()` / `send()` →
    `wasi:http/types@0.2.3.[constructor]outgoing-request` +
    `wasi:http/outgoing-handler@0.2.3#handle` (v0.16, this slice).

  Each of these splices the versioned P2 interface into the
  core module's import section verbatim — no adapter hop needed.
- **shim-routed P2** (deprecated for v0.17): `log()` / `print()`.
  Mighty's slice-8 emitter declares `wasi:cli/log#log` as an
  unversioned import; the P2 wrap path declares a matching
  unversioned `wasi:cli` package containing only that shim so
  `wit-component::ComponentEncoder` can resolve it. The shim's
  WIT carries a `// DEPRECATED:` comment flagging the v0.17
  migration plan: route to
  `wasi:cli/stdout@0.2.3#get-stdout` +
  `wasi:io/streams@0.2.3#output-stream.blocking-write-and-flush`.

Both transports produce a P2-compliant component — the host sees
versioned `wasi:*@0.2.3` imports either way. The adapter is
still embedded so the `log()` shim resolves; once v0.17 lands the
final direct lowering, the adapter becomes opt-in.

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
(the full upstream wasi-cli + wasi-http surface, concatenated). The
vendored P1→P2 adapter modules live alongside in
`crates/mty-codegen-wasm/adapter/` and are sourced from wasmtime
v32.0.0 — pinned to the same WASI version as the WIT. To upgrade
both in lockstep, follow the procedure in
`crates/mty-codegen-wasm/adapter/README.md`.

`mty_codegen_wasm::WASI_P1_ADAPTER_VERSION` exposes the wasmtime
release tag for the vendored adapter (e.g. `"wasmtime-v32.0.0"`).

## Roadmap

- v0.14 (shipped): vendored P1→P2 adapter embedded by default;
  `std.random` + `std.time` direct-import helpers landed as
  constants; emitter still routed those calls through the
  Unsupported fallback.
- v0.15 (shipped): direct-import dispatch wired through emit.rs for
  `std.random.bytes` + `std.time.{now,monotonic_now,resolution}`;
  `--wasi=p2` is now the default for `wasm32-wasi`; `--wasi=p1`
  remains supported for back-compat.
- v0.16 (shipped, this slice): direct lowering for
  `std.fs.{open,read_file,write_file,stat,close}` and
  `std.http.{get,post,send}`; canonical-ABI helpers
  (`emit_resource_drop_call` / per-variant signatures);
  pre-decl pass in the emitter to keep function indices stable
  when the lazy import-declaration adds a new import mid-body.
- v0.17: direct lowering for `log()` (replaces the
  `wasi:cli/log` shim with a real `wasi:cli/stdout@0.2.3` +
  `wasi:io/streams@0.2.3` lift); full resource-handle lifecycle
  for `std.fs` (open + close scaffold around read/write/stat);
  full streaming layer for `std.http` (subscribe + poll loop on
  `future-incoming-response`); adapter becomes opt-in for builds
  that avoid all P1 syscalls.
- v1.0 RC4: P1 becomes a tier-2 target (still emitted on request but
  no longer the default; the documentation tree assumes P2).

See `dev/history/notes/WASI_P2_V0_13_NOTES.md` for the v0.13
plan + open decisions,
`dev/history/notes/WASI_P2_LOWERINGS_V0_14_NOTES.md` for the v0.14
follow-up, `dev/history/notes/WASI_P2_FINISH_V0_15_NOTES.md` for
the v0.15 default-flip + emitter-wiring rationale, and
`dev/history/notes/WASI_P2_FS_HTTP_V0_16_NOTES.md` for the v0.16
fs + http direct-lowering rationale.
