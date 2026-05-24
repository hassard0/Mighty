# Getting Started

This page walks through installing the Stardust compiler, scaffolding a
package, and running the `sdust` CLI against your first program.

## Install

There is no binary release yet. Build the compiler from source with a
recent Rust toolchain (1.85+; slice 8 bumped MSRV).

```bash
git clone https://github.com/hassard0/stardust
cd stardust
cargo install --path crates/sdust-cli
```

This places `sdust` on your `PATH`. Verify with:

```bash
sdust --version
```

## Scaffold a package

```bash
sdust new hello
```

This creates:

```
hello/
├── star.toml
└── src/
    └── main.sd
```

`star.toml` is the package manifest. The generated file is minimal:

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
  log("hello, Stardust")
}
```

See [reference/manifest.md](reference/manifest.md) for the full manifest
schema.

## Check it

```bash
cd hello
sdust check src/main.sd
```

`sdust check` parses the source, builds the CST and HIR, runs the type
checker, the effect / capability checker, and the borrow checker, and
prints any diagnostics. On success it prints `ok: <path>` and exits 0.

## Run it

```bash
sdust run src/main.sd
```

`sdust run` runs the full `check` pipeline, lowers the program to SIR,
JIT-compiles via Cranelift, and invokes `main`. Programs whose SIR
the slice-8 backend can't yet lower fall back to the slice-7 runtime
(tokio executor + per-turn interpreter) transparently. The above
program prints `hello, Stardust` and exits 0. See
[reference/cli/sdust-run.md](reference/cli/sdust-run.md) for details on
exit codes, traps, and the effect-handling model.

## Build it

```bash
sdust build src/main.sd
# → wrote target/hello   (or hello.exe on Windows)

sdust build --target wasm32-wasi src/main.sd
# → wrote target/hello.wasm
```

`sdust build` produces a real, runnable artifact. The native target
uses Cranelift to emit a host-format `.o`, then links via the
platform C linker (`clang` / `gcc` / `cc`). If no linker is on
PATH, the `.o` is left in `target/` and a helpful message tells you
how to link manually. The Wasm target produces a core Wasm module
runnable under `wasmtime`/`wasmer` or a browser host.

See [reference/cli/sdust-build.md](reference/cli/sdust-build.md) for
flags, exit codes, and the v0.1 backend coverage matrix (slice-8
native + wasm cover a narrow SIR subset; richer programs will
require the v0.2 LLVM backend or fall back to `sdust run`).

## Format

```bash
sdust fmt src/main.sd
```

The slice 1 formatter is an identity pass: it re-emits the source verbatim
once it has confirmed the source parses. Real per-node formatting lands in
slice 2.

Pass `--check` to verify formatting without writing, or `--stdin` to read
from standard input.

## Inspect intermediate forms

```bash
sdust dump --cst src/main.sd
sdust dump --ast src/main.sd
sdust dump --hir src/main.sd
sdust dump --sir src/main.sd
```

Use these to debug parser/lowering behavior or to write your own tooling
against the compiler. See [reference/cli/sdust-dump.md](reference/cli/sdust-dump.md).

## Your first agent

The smallest interesting program in Stardust is an agent. Create
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
sdust check src/echo.sd
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
- Browse the [CLI reference](reference/cli/sdust.md).
- Check the [FAQ](faq.md) for the most common questions.
