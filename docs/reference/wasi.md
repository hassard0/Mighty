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
| `log()` / `print()` | imports `wasi:cli/log` | imports `mighty:cli-adapter/log` (shim) |
| `std.fs.read()` | adapter shim | declared `wasi:filesystem` import (lowering TODO) |
| `std.fs.write()` | adapter shim | declared `wasi:filesystem` import (lowering TODO) |
| `std.http.get()` | adapter shim | declared `wasi:http/outgoing-handler` import (lowering TODO) |
| `std.time.now()` | not yet wired | `wasi:clocks/monotonic-clock` (lowering TODO) |
| `std.random.*` | not yet wired | `wasi:random/random` (lowering TODO) |
| User-WIT `[wit]` section | ignored | merged into world |
| `--world <name>` | ignored | picks world from user WIT |

The "lowering TODO" entries mean the produced component **declares**
the import in its WIT contract (so a P2-compliant host can wire it),
but Mighty's codegen doesn't yet emit calls that exercise it. The
v0.14 roadmap closes those gaps; see `WASI_P2_V0_13_NOTES.md`.

## How `--wasi=p2` changes the output

A P2 build does three things differently from the default P1 path:

1. The generated WIT document imports versioned P2 interfaces:
   `wasi:cli@0.2.3`, `wasi:io@0.2.3`, `wasi:clocks@0.2.3`,
   `wasi:filesystem@0.2.3`, `wasi:http@0.2.3`, `wasi:random@0.2.3`.
2. The component's package id becomes `mighty:<pkg>` (instead of
   `stardust:<pkg>`).
3. The shim package `mighty:cli-adapter` is declared so the core
   module's existing `wasi:cli/log#log` import keeps wiring through
   `wit-component::ComponentEncoder` without modification.

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

Today: **none**. Every `std.*` call still lowers to the P1 adapter
import shape. The P2 path makes the component **declare** the right
P2 imports so a P2 host accepts instantiation, but the core module
still emits P1-style calls. We document this gap rather than fail
silently so users can audit which modules are safe to use under a
strict P2 host.

The v0.14 plan flips this — `std.fs`, `std.http`, `std.time`,
`std.random` will lower directly to P2 import calls and the adapter
shim disappears. At that point P2 becomes the default for new
projects; P1 stays available behind `--wasi=p1` for the migration
window.

## Versioning

Mighty v0.13 targets **WASI 0.2.3**. The exact version string is
exposed as `mty_codegen_wasm::WASI_P2_VERSION`. The vendored P2 WIT
slice lives at `crates/mty-codegen-wasm/wit/wasi-p2/wasi-p2.wit`;
swap it for an upstream `wit/deps/` tree if you need a newer or
older P2.

## Roadmap

- v0.14: flip `std.*` lowering to call P2 imports directly; remove
  the `mighty:cli-adapter` shim; make `--wasi=p2` the default for
  `wasm32-wasi`.
- v1.0 RC4: P1 becomes a tier-2 target (still emitted on request but
  no longer the default; the documentation tree assumes P2).

See `dev/history/notes/WASI_P2_V0_13_NOTES.md` for the slice-by-slice
plan and the open decisions.
