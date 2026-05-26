# WASI Preview 1 → Preview 2 adapter modules

This directory vendors the official `wasi_snapshot_preview1`
adapter modules built by the [Bytecode Alliance][bca]
[`wasmtime`][wasmtime] project. They are *core* Wasm modules
which export the legacy `wasi_snapshot_preview1` interface in
terms of versioned WASI Preview 2 (0.2.x) interfaces, and are
passed to `wit_component::ComponentEncoder::adapter()` so a core
module emitting P1-shaped imports can be wrapped into a P2
component without rewriting the core.

## Files

| File | Use | Approx size |
|------|-----|-------------|
| `wasi_snapshot_preview1.command.wasm` | For *command* components (programs with a `main` export — the default for `mty build`) | ~54 KB |
| `wasi_snapshot_preview1.reactor.wasm` | For *reactor* components (no `main`, e.g. libraries that export functions to be called by the host) | ~54 KB |
| `wasi_snapshot_preview1.proxy.wasm` | For *proxy* components (`wasi-http` style — input is a request, output is a response) | ~18 KB |

Mighty v0.14 only embeds `command.wasm` (every `mty build` produces
a command-shaped component). The reactor + proxy variants are
vendored alongside it so future slices that emit those component
shapes don't need a second adapter-vendoring pass.

## Provenance

These bytes were downloaded directly from the wasmtime v32.0.0
GitHub release on 2026-05-25:

```
https://github.com/bytecodealliance/wasmtime/releases/download/v32.0.0/wasi_snapshot_preview1.command.wasm
https://github.com/bytecodealliance/wasmtime/releases/download/v32.0.0/wasi_snapshot_preview1.reactor.wasm
https://github.com/bytecodealliance/wasmtime/releases/download/v32.0.0/wasi_snapshot_preview1.proxy.wasm
```

Wasmtime v32.0.0 is the first stable release whose adapter targets
WASI **0.2.3** — the version Mighty's vendored P2 WIT slice
declares (`crates/mty-codegen-wasm/wit/wasi-p2/wasi-p2.wit`).
Earlier wasmtime releases shipped 0.2.0 / 0.2.2 adapters whose
imports don't match our WIT contract (the `wit-component`
encoder rejects a semver-incompatible "upgrade" mid-encoding).
If we bump the WIT slice to a newer 0.2.x, also bump the adapter
version here (and update the table above).

## Verification

The adapters are MVP-version Wasm core modules:

```
$ file wasi_snapshot_preview1.command.wasm
WebAssembly (wasm) binary module version 0x1 (MVP)
```

You can dump the imports/exports with `wasm-tools`:

```bash
wasm-tools print wasi_snapshot_preview1.command.wasm | head
```

The exports include the full `wasi_snapshot_preview1` surface
(`fd_read`, `fd_write`, `path_open`, `clock_time_get`, …). The
imports are versioned P2 interfaces (`wasi:io/streams@0.2.x`,
`wasi:filesystem/types@0.2.x`, `wasi:clocks/wall-clock@0.2.x`, …).

## License

Wasmtime — and therefore these adapter bytes — is dual-licensed
under Apache-2.0 (with LLVM exception) and MIT. See the upstream
[`LICENSE`][wasmtime-license] for full text. Both licenses are
compatible with Mighty's MIT license; vendoring the bytes
verbatim is permitted under the Apache-2.0 source-redistribution
clause provided this notice is preserved.

## Why vendor at all?

The alternative is to fetch the adapter at build time (e.g. via
`build.rs`), but:

- It introduces a network dependency on `cargo build`, which
  breaks offline + reproducible builds.
- The adapter is small (~80 KB compressed in-binary), and
  Cargo's `include_bytes!` macro means it doesn't slow down
  recompiles.
- Vendoring lets us pin the exact adapter version against our
  `wit-component` version with a single PR.

The adapter version we ship is a *Mighty release artifact* — we
intentionally don't track upstream wasmtime master.

[bca]: https://bytecodealliance.org/
[wasmtime]: https://github.com/bytecodealliance/wasmtime
[wasmtime-license]: https://github.com/bytecodealliance/wasmtime/blob/main/LICENSE
