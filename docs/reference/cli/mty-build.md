# `mty build`

Compile a Mighty source file to a runnable artifact (slice 8).

## Synopsis

```
mty build <PATH> [--debug | --release] [--target TARGET] [--out-dir DIR] [--no-component]
```

## Description

`mty build` runs the same parse → lower → typeck → borrowck →
MtyIR pipeline as `mty run`, then hands the program to the
configured backend. Two backends ship in v0.1:

- **native** (default) — Cranelift via `mty-codegen-cranelift`.
  Produces a host-format object (`.o`) and links it into an
  executable via the platform linker.
- **wasm32-wasi** / **wasm32-web** — `mty-codegen-wasm`. Emits a
  core Wasm module (`.wasm`).

Per [A46](../../spec/v0.1-amendments.md#a46), LLVM is not the
default native backend in v0.1; it's a scaffold behind a feature
flag.

## Arguments

| Flag | Description |
|------|-------------|
| `<PATH>` | Path to a `.sd` source file. |
| `--debug` | Build in debug mode (default). Smaller compile time, no optimization, *debug info emitted* (DWARF on native; `name` + `.wasm.map` on wasm). |
| `--release` | Build in release mode. Cranelift `opt_level = speed`. Debug info stripped. |
| `--target <TARGET>` | One of `native` (default), `wasm32-wasi`, `wasm32-web`. |
| `--out-dir <DIR>` | Output directory. Default `target/`. |
| `--no-component` | Wasm targets only: emit a bare core wasm module instead of a Component Model component. Useful for runtimes that don't yet support the Component Model, or for debugging the lowering. Default = Component Model output (v0.2 wave-2, closes [A47](../../spec/v0.1-amendments.md#a47)). |

## Output

Native target: writes `<DIR>/<name>` (or `<name>.exe` on Windows).
Intermediate object preserved at `<DIR>/<name>.o`.

Wasm target: writes `<DIR>/<name>.wasm`. No linker step.

By default the emitted bytes are a Component Model component
(preamble `\0asm\x0d\x00\x01\x00`). With `--no-component`, a bare
core wasm module is written instead (preamble
`\0asm\x01\x00\x00\x00`).

`WasmArtifact::wit_text` carries the generated WIT contract in
both modes — downstream tools can read it from the artifact
metadata.

The binary's name is derived from the source file's stem
(`examples/01_hello.sd` → `01_hello`).

## Linker discovery (A52)

For native builds, the discovery order is:

1. `$MTY_LINKER` env var, if set and non-empty (legacy
   `$STARDUST_LINKER` is still honoured with a one-shot
   deprecation warning).
2. `clang` (`clang.exe` on Windows).
3. `gcc` (`gcc.exe`).
4. `cc` (`cc.exe`).

The MSYS/Git-Bash `/usr/bin/link.exe` shim is **skipped** because
it's a hardlink helper, not a linker.

If no linker is found, `mty build` still succeeds for the object
file. The exit message is:

```
wrote object target/<name>.o (no linker found; set $MTY_LINKER)
```

You can then invoke the linker yourself:

```bash
clang target/01_hello.o -o target/01_hello
./target/01_hello
```

## Examples

```bash
# Default: native debug build → target/01_hello (or .exe)
mty build examples/01_hello.sd

# Native release build with a custom out dir:
mty build --release --out-dir dist/ src/main.sd

# Wasm WASI build → Component Model component (v0.2 default):
mty build --target wasm32-wasi examples/01_hello.sd
# Wasmtime requires the component-model flag:
wasmtime --wasm component-model target/01_hello.wasm

# Bare core wasm module (skip component wrapper):
mty build --no-component --target wasm32-wasi examples/01_hello.sd
wasmtime target/01_hello.wasm                          # works without --wasm component-model

# Browser-targeted Wasm → Component Model, transpile via jco:
mty build --target wasm32-web src/widget.sd
jco transpile target/widget.wasm -o dist/widget       # emits ESM glue + .wasm core
# then load dist/widget/widget.js as a module in your page
```

## Debug info

`--debug` builds embed debug info into the artifact (the flag is on by
default; pass `--release` to strip):

- **Native objects** carry standard DWARF v4 sections (`.debug_info`,
  `.debug_abbrev`, `.debug_line`, `.debug_str`). Use `lldb`, `gdb`, or
  `objdump --dwarf=info` to inspect.
- **Wasm modules** gain a `name` custom section (function names) plus
  a `sourceMappingURL` custom section pointing at the sidecar
  `<binary>.wasm.map` (source-map v3 JSON). DevTools / Chrome load
  the sidecar automatically; the Component Model wrapper preserves
  both custom sections.

See [docs/internals/debug-info.md](../../internals/debug-info.md) for
the v0.2 coverage matrix, format details, and known limitations
(coarse line table, no `.debug_loc` location lists yet).

```bash
mty build --debug examples/01_hello.sd
objdump --dwarf=info target/01_hello.o   # native DWARF

mty build --debug --target wasm32-wasi examples/01_hello.sd
ls target/01_hello.wasm target/01_hello.wasm.map
```

### Wasm runtime compatibility

| Runtime | Component default | `--no-component` core module |
|---------|--------------------|------------------------------|
| `wasmtime --wasm component-model` | ✅ | n/a |
| `wasmtime` (plain) | ❌ — need flag | ✅ |
| `wasmer` ≥ 4.3 | ✅ | ✅ |
| `wasmer` < 4.3 | ❌ | ✅ |
| Browser via `jco transpile` | ✅ | n/a (use component) |
| `wasm-tools component validate` | ✅ | n/a (it's not a component) |

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Build succeeded; artifact written. |
| 1 | Frontend error (parse / typeck / borrowck). Diagnostics already rendered. |
| 2 | Backend or CLI error (unknown target, codegen rejection, link failure). Error message printed to stderr. |

## Backend coverage matrix

The slice-8 native and wasm backends cover a deliberately narrow
MtyIR subset. Programs the backend can't lower trigger
`CodegenError::Unsupported(reason)`. For `mty run` this falls
back to the interpreter transparently; for `mty build` it
surfaces as exit 2 with a `build error: ...` message.

See [SLICE8.md](https://github.com/hassard0/Mighty/blob/main/SLICE8.md) for the per-example matrix and
the v0.2 backlog.

## See also

- `mty run` — JIT-compile and execute in one step
- `mty check` — type / borrow check without lowering
- `mty dump` — emit AST / CST / HIR / MtyIR for inspection
- [Codegen internals: Cranelift](../../internals/codegen-cranelift.md)
- [Codegen internals: Wasm](../../internals/codegen-wasm.md)
- [Codegen internals: LLVM scaffold](../../internals/codegen-llvm.md)
