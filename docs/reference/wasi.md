# WASI compatibility

Mighty targets the WebAssembly Component Model on every `mty build
--target wasm32-*` invocation. Through v0.12 the only host shape was
**WASI Preview 1** (P1) — the imports the slice-8 emitter wires
(`wasi:cli/log#log`) match the P1 snapshot interface.

v0.13 introduces an opt-in **WASI Preview 2** (P2) backend driven by
the `--wasi=p2` flag. This page describes both modes, the
compatibility matrix, and how to consume user-authored `.wit` files.

## TL;DR

```bash
# Default — keeps the v0.2..v0.12 P1 shape.
mty build hello.mty --target wasm32-wasi

# Opt into the v0.13 P2 backend.
mty build hello.mty --target wasm32-wasi --wasi=p2

# P2 + user-supplied WIT (worlds defined in mighty.toml `[wit]`).
mty build hello.mty --target wasm32-wasi --wasi=p2 --world my-world
```

## Compatibility matrix

| Mighty surface | P1 (default) | P2 (`--wasi=p2`) |
|----------------|--------------|------------------|
| `log()` / `print()` | imports `wasi:cli/log` | imports `wasi:cli/log` (unversioned shim) — v0.15 replaces with `wasi:cli/stdout@0.2.3` |
| `std.fs.read()` / `write()` | P1 syscall | P1 syscall, **translated to `wasi:filesystem@0.2.3` by the vendored adapter** (v0.15 adds direct lowering) |
| `std.http.get()` / `post()` | P1 syscall | P1 syscall, **translated to `wasi:http@0.2.3` by the vendored adapter** (v0.15 adds direct lowering) |
| `std.time.now()` / `monotonic_now()` / `resolution()` | P1 syscall | **direct P2 import** `wasi:clocks/{wall-clock,monotonic-clock}@0.2.3` (v0.14) |
| `std.random.bytes()` / `u64()` | P1 syscall | **direct P2 import** `wasi:random/random@0.2.3` (v0.14) |
| `wasi_snapshot_preview1` adapter | n/a | **embedded** (~54 KB) — wasmtime v32.0.0 command shape |
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

The v0.15 plan flips `std.fs` + `std.http` + `log()` to the
direct path, after which the adapter becomes optional and can be
opted out via `Preview2Options::with_adapter(None)` for smaller
components.

## How `--wasi=p2` changes the output

A P2 build does five things differently from the default P1 path:

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

v0.14 status:

- **direct P2 lowering**: `std.time.now()`,
  `std.time.monotonic_now()`, `std.time.resolution()`,
  `std.random.bytes()`, `std.random.u64()`. The core module
  imports the versioned P2 interface (e.g.
  `wasi:random/random@0.2.3#get-random-bytes`) verbatim — no
  adapter hop needed.
- **adapter-routed P2**: `std.fs.*`, `std.http.*`, `log()`. The
  core module still imports the P1 syscall (e.g.
  `wasi_snapshot_preview1#fd_write`); the vendored adapter
  translates that into the matching `wasi:filesystem@0.2.3`
  call at instantiation.

Both transports produce a P2-compliant component — the host sees
versioned `wasi:*@0.2.3` imports either way. The difference is the
~54 KB of adapter Wasm embedded into the component when any
adapter-routed surface is used.

The v0.15 plan flips the remaining surfaces (`std.fs`, `std.http`,
`log()`) to direct lowering, at which point the adapter becomes
opt-in. P2 will become the default for `wasm32-wasi` once all four
surfaces lower directly.

## Versioning

Mighty v0.14 targets **WASI 0.2.3**. The exact version string is
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
  `std.random` + `std.time` lower to direct P2 imports.
- v0.15: direct lowering for `std.fs`, `std.http`, `log()`; adapter
  becomes opt-in; `--wasi=p2` becomes the default for `wasm32-wasi`.
- v1.0 RC4: P1 becomes a tier-2 target (still emitted on request but
  no longer the default; the documentation tree assumes P2).

See `dev/history/notes/WASI_P2_V0_13_NOTES.md` for the v0.13
plan + open decisions and
`dev/history/notes/WASI_P2_LOWERINGS_V0_14_NOTES.md` for the v0.14
follow-up.
