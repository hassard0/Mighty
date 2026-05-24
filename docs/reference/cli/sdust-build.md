# `sdust build`

Compile a Stardust source file to a runnable artifact (slice 8).

## Synopsis

```
sdust build <PATH> [--debug | --release] [--target TARGET] [--out-dir DIR]
```

## Description

`sdust build` runs the same parse → lower → typeck → borrowck →
SIR pipeline as `sdust run`, then hands the program to the
configured backend. Two backends ship in v0.1:

- **native** (default) — Cranelift via `sdust-codegen-cranelift`.
  Produces a host-format object (`.o`) and links it into an
  executable via the platform linker.
- **wasm32-wasi** / **wasm32-web** — `sdust-codegen-wasm`. Emits a
  core Wasm module (`.wasm`).

Per [A46](../../spec/v0.1-amendments.md#a46), LLVM is not the
default native backend in v0.1; it's a scaffold behind a feature
flag.

## Arguments

| Flag | Description |
|------|-------------|
| `<PATH>` | Path to a `.sd` source file. |
| `--debug` | Build in debug mode (default). Smaller compile time, no optimization. |
| `--release` | Build in release mode. Cranelift `opt_level = speed`. |
| `--target <TARGET>` | One of `native` (default), `wasm32-wasi`, `wasm32-web`. |
| `--out-dir <DIR>` | Output directory. Default `target/`. |

## Output

Native target: writes `<DIR>/<name>` (or `<name>.exe` on Windows).
Intermediate object preserved at `<DIR>/<name>.o`.

Wasm target: writes `<DIR>/<name>.wasm`. No linker step.

The binary's name is derived from the source file's stem
(`examples/01_hello.sd` → `01_hello`).

## Linker discovery (A52)

For native builds, the discovery order is:

1. `$STARDUST_LINKER` env var, if set and non-empty.
2. `clang` (`clang.exe` on Windows).
3. `gcc` (`gcc.exe`).
4. `cc` (`cc.exe`).

The MSYS/Git-Bash `/usr/bin/link.exe` shim is **skipped** because
it's a hardlink helper, not a linker.

If no linker is found, `sdust build` still succeeds for the object
file. The exit message is:

```
wrote object target/<name>.o (no linker found; set $STARDUST_LINKER)
```

You can then invoke the linker yourself:

```bash
clang target/01_hello.o -o target/01_hello
./target/01_hello
```

## Examples

```bash
# Default: native debug build → target/01_hello (or .exe)
sdust build examples/01_hello.sd

# Native release build with a custom out dir:
sdust build --release --out-dir dist/ src/main.sd

# Wasm preview1 build:
sdust build --target wasm32-wasi examples/01_hello.sd
wasmtime target/01_hello.wasm

# Browser-targeted Wasm:
sdust build --target wasm32-web src/widget.sd
# load target/widget.wasm in a <script type="module"> Worker
```

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Build succeeded; artifact written. |
| 1 | Frontend error (parse / typeck / borrowck). Diagnostics already rendered. |
| 2 | Backend or CLI error (unknown target, codegen rejection, link failure). Error message printed to stderr. |

## Backend coverage matrix

The slice-8 native and wasm backends cover a deliberately narrow
SIR subset. Programs the backend can't lower trigger
`CodegenError::Unsupported(reason)`. For `sdust run` this falls
back to the interpreter transparently; for `sdust build` it
surfaces as exit 2 with a `build error: ...` message.

See [SLICE8.md](../../../SLICE8.md) for the per-example matrix and
the v0.2 backlog.

## See also

- `sdust run` — JIT-compile and execute in one step
- `sdust check` — type / borrow check without lowering
- `sdust dump` — emit AST / CST / HIR / SIR for inspection
- [Codegen internals: Cranelift](../../internals/codegen-cranelift.md)
- [Codegen internals: Wasm](../../internals/codegen-wasm.md)
- [Codegen internals: LLVM scaffold](../../internals/codegen-llvm.md)
