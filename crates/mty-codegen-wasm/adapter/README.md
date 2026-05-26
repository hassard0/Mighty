# WASI Preview 1 → Preview 2 adapter modules — REMOVED in v0.19

This directory used to vendor the official `wasi_snapshot_preview1`
adapter modules built by the [Bytecode Alliance][bca]
[`wasmtime`][wasmtime] project (v32.0.0). The bytes were embedded into
`mty-codegen-wasm` via `include_bytes!` so any P2 build could ship the
P1→P2 hop without a network round-trip.

The vendored bytes are **gone** as of v0.19. The directory is kept so
the `include_bytes!` paths in the v0.13–v0.18 history compile against
checkouts of older tags, but no `.wasm` files live here today.

## Why?

- v0.15–v0.17 added direct P2 lowerings for every stdlib syscall
  Mighty emits (`random`, `time`, `fs`, `http`, `log`). The default
  Mighty build does not import `wasi_snapshot_preview1` at all.
- v0.17 flipped `Preview2Options::default().embed_adapter` to `None`.
  The vendored bytes were already opt-in.
- v0.19 finishes the cleanup by removing the dead 150 KB of bytes
  from every `cargo build` of the crate (3 files × ~50 KB).

## Need an adapter?

Callers that link a `wasi-libc`-built C crate (or otherwise import
`wasi_snapshot_preview1` directly from the core module) can still opt
in via:

```rust
use mty_codegen_wasm::{AdapterEmbed, AdapterKind, Preview2Options};

let bytes = std::fs::read("/path/to/wasi_snapshot_preview1.command.wasm")?;
let opts = Preview2Options::new("my-pkg")
    .with_adapter(Some(AdapterEmbed::new(AdapterKind::Command, bytes)));
```

The bytes themselves are an upstream release artifact — download the
adapter matching the WASI version your build targets from the
corresponding [wasmtime release][wasmtime-releases]:

```
https://github.com/bytecodealliance/wasmtime/releases/download/<TAG>/wasi_snapshot_preview1.command.wasm
https://github.com/bytecodealliance/wasmtime/releases/download/<TAG>/wasi_snapshot_preview1.reactor.wasm
https://github.com/bytecodealliance/wasmtime/releases/download/<TAG>/wasi_snapshot_preview1.proxy.wasm
```

Mighty v0.18 targeted WASI 0.2.3, which first stabilized in
wasmtime v32.0.0; bump in lockstep with whatever WIT slice
`crates/mty-codegen-wasm/wit/wasi-p2/wasi-p2.wit` declares.

## License

Wasmtime — and the adapter bytes it ships — is dual-licensed under
Apache-2.0 (with LLVM exception) and MIT. See the upstream
[`LICENSE`][wasmtime-license] for full text. Both licenses are
compatible with Mighty's MIT license; redistribute the bytes verbatim
when you vendor them in your own tree.

[bca]: https://bytecodealliance.org/
[wasmtime]: https://github.com/bytecodealliance/wasmtime
[wasmtime-releases]: https://github.com/bytecodealliance/wasmtime/releases
[wasmtime-license]: https://github.com/bytecodealliance/wasmtime/blob/main/LICENSE
