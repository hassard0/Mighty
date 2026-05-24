# Getting Started

This page walks through installing the Mighty compiler, scaffolding a
package, and running the `mty` CLI against your first program.

## Install

There is no binary release yet. Build the compiler from source with a
recent Rust toolchain (1.85+; slice 8 bumped MSRV).

```bash
git clone https://github.com/hassard0/stardust
cd mighty
cargo install --path crates/mty-cli
```

This places `mty` on your `PATH`. Verify with:

```bash
mty --version
```

## Scaffold a package

```bash
mty new hello
```

This creates:

```
hello/
├── mighty.toml
└── src/
    └── main.sd
```

`mighty.toml` is the package manifest. The generated file is minimal:

```toml
[package]
name = "hello"
version = "0.1.0"
edition = "2026"
profile = "host"

[deps]
```

`src/main.sd` is the entry point:

```sd
fn main() {
  log("hello, Mighty")
}
```

See [reference/manifest.md](reference/manifest.md) for the full manifest
schema.

## Check it

```bash
cd hello
mty check src/main.sd
```

`mty check` parses the source, builds the CST and HIR, runs the type
checker, the effect / capability checker, and the borrow checker, and
prints any diagnostics. On success it prints `ok: <path>` and exits 0.

## Run it

```bash
mty run src/main.sd
```

`mty run` runs the full `check` pipeline, lowers the program to MtyIR,
JIT-compiles via Cranelift, and invokes `main`. Programs whose MtyIR
the slice-8 backend can't yet lower fall back to the slice-7 runtime
(tokio executor + per-turn interpreter) transparently. The above
program prints `hello, Mighty` and exits 0. See
[reference/cli/mty-run.md](reference/cli/mty-run.md) for details on
exit codes, traps, and the effect-handling model.

## Build it

```bash
mty build src/main.sd
# → wrote target/hello   (or hello.exe on Windows)

mty build --target wasm32-wasi src/main.sd
# → wrote target/hello.wasm
```

`mty build` produces a real, runnable artifact. The native target
uses Cranelift to emit a host-format `.o`, then links via the
platform C linker (`clang` / `gcc` / `cc`). If no linker is on
PATH, the `.o` is left in `target/` and a helpful message tells you
how to link manually. The Wasm target produces a core Wasm module
runnable under `wasmtime`/`wasmer` or a browser host.

See [reference/cli/mty-build.md](reference/cli/mty-build.md) for
flags, exit codes, and the v0.1 backend coverage matrix (slice-8
native + wasm cover a narrow MtyIR subset; richer programs will
require the v0.2 LLVM backend or fall back to `mty run`).

## Test

```bash
mty-test
```

`mty-test` walks `tests/` in the current package, runs every
`fn test_*` it finds via the MtyIR interpreter, and prints a
`cargo test`-style report. Exit code: 0 on all-pass, 1 on any
failure. Pass `--dir <path>` to test a directory other than
`tests/`. See [reference/stdlib/test.md](reference/stdlib/test.md)
for the full discovery + execution model.

In v0.3 this merges into the main `mty` CLI as `mty test`.

## Format

```bash
mty fmt src/main.sd
```

The slice 1 formatter is an identity pass: it re-emits the source verbatim
once it has confirmed the source parses. Real per-node formatting lands in
slice 2.

Pass `--check` to verify formatting without writing, or `--stdin` to read
from standard input.

## Inspect intermediate forms

```bash
mty dump --cst src/main.sd
mty dump --ast src/main.sd
mty dump --hir src/main.sd
mty dump --sir src/main.sd
```

Use these to debug parser/lowering behavior or to write your own tooling
against the compiler. See [reference/cli/mty-dump.md](reference/cli/mty-dump.md).

## Your first agent

The smallest interesting program in Mighty is an agent. Create
`src/echo.sd`:

```sd
protocol Echo {
  Ping(msg: Str) -> Str
}

agent Echoer: Echo {
  on Ping(msg) -> msg
}
```

Then:

```bash
mty check src/echo.sd
```

`protocol` declares a typed message contract. `agent` declares a unit of
state, concurrency, and failure that implements one or more protocols. The
`on Ping(msg) -> msg` handler is the compact form of:

```sd
on Ping(msg) {
  return msg
}
```

Continue with the [tour](tour/README.md) to see how these primitives
compose into real programs.

## Next steps

- Work through the [tour](tour/README.md) chapter by chapter.
- Read the [language specification](spec/v0.1.md).
- Browse the [CLI reference](reference/cli/mty.md).
- Check the [FAQ](faq.md) for the most common questions.
