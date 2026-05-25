# Getting Started

This page walks through installing the Mighty compiler, scaffolding a
package, running your first program, writing your first agent, and
running your first test.

> **Pre-alpha warning.** Mighty is at **v0.10** (toolchain) tracking
> spec **v1.0-RC2**. The language surface is frozen for v1.0 but
> there is no binary release yet, no stability guarantee on internal
> APIs, and several DEFER-V1.1 amendments (RFCs 001..006) are still
> open. Treat it as "ready to play with", **not** "ready for
> production".

## 1. Install

Build the compiler from source with a recent Rust toolchain.

- **MSRV:** Rust 1.85 (slice 8 bumped the MSRV).
- **Platforms:** Linux, macOS (Intel + Apple Silicon), Windows.
- **Dependencies:** a C linker (`clang` / `gcc` on \*nix, MSVC's
  `link.exe` on Windows) only if you plan to use `mty build` with
  the native target. `mty run` and `mty check` need only Rust.

```bash
git clone https://github.com/hassard0/Mighty
cd Mighty
cargo install --path crates/mty-cli
```

This places `mty` on your `PATH`. Verify with:

```bash
mty --version
```

Expected output:

```
mty 0.10.0
```

On Windows, if `cargo install` fails with a linker error, see the
[FAQ entry on the Windows DLL gotcha](faq.md#why-does-mty-fail-to-link-on-windows).
On macOS, if your build fails with a `LC_BUILD_VERSION` warning, see
the [macOS note](faq.md#what-is-the-macos-lc_build_version-fix).

## 2. Scaffold a package

```bash
mty new hello
```

This creates:

```
hello/
├── mighty.toml
└── src/
    └── main.mty
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

`src/main.mty` is the entry point:

```mty
fn main() {
  log("hello, Mighty")
}
```

See [reference/manifest.md](reference/manifest.md) for the full
manifest schema. The default profile is `host`; for embedded-style
work see the FAQ entry on the `core` profile.

## 3. Check it

```bash
cd hello
mty check src/main.mty
```

`mty check` parses the source, builds the CST and HIR, runs the
type checker, the effect / capability checker, and the borrow
checker, and prints any diagnostics. On success it prints `ok:
<path>` and exits 0.

```
ok: src/main.mty
```

If anything fails, the diagnostic prints with file, line, column,
and a stable diagnostic code (`MTxxxx`). Run `mty explain MTxxxx`
for a Cause/Example/Fix/Spec block for that specific code. For
example:

```bash
mty explain MT2001
```

```
MT2001: Type mismatch.

Cause:   An expression's type does not match the type required by
         context (parameter, annotation, branch unification, or
         return type).
Example: `fn f() -> I32 { "hello" }`  // returns Str, not I32
Fix:     Convert the value (`.to_string()`, `.parse()`, an
         explicit constructor), or change the annotation. ...
Spec:    §7.2 (unification) of v1.0-RC2.
```

## 4. Run it

```bash
mty run src/main.mty
```

`mty run` runs the full `check` pipeline, lowers the program to
MtyIR, JIT-compiles via Cranelift, and invokes `main`. Programs
whose MtyIR the native backend can't yet lower fall back to the
v0.7 interpreter (tokio executor + per-turn evaluator)
transparently. The above program prints:

```
hello, Mighty
```

and exits 0. See [reference/cli/mty-run.md](reference/cli/mty-run.md)
for details on exit codes, traps, and the effect-handling model.

## 5. Build it

```bash
mty build src/main.mty
# → wrote target/hello   (or hello.exe on Windows)

mty build --target wasm32-wasi src/main.mty
# → wrote target/hello.wasm
```

`mty build` produces a real, runnable artifact. The native target
uses Cranelift to emit a host-format `.o`, then links via the
platform C linker (`clang` / `gcc` / `cc` / `link.exe`). If no
linker is on PATH, the `.o` is left in `target/` and a helpful
message tells you how to link manually (see `MT8008`).

The Wasm target produces a core Wasm module runnable under
`wasmtime` / `wasmer` or any browser host.

See [reference/cli/mty-build.md](reference/cli/mty-build.md) for the
full flag list and current backend coverage matrix.

## 6. Your first agent

The smallest interesting program in Mighty is an agent. Create
`src/echo.mty`:

```mty
protocol Echo {
  Ping(msg: Str) -> Str
}

agent Echoer: Echo {
  on Ping(msg) -> msg
}
```

Then:

```bash
mty check src/echo.mty
```

Output:

```
ok: src/echo.mty
```

`protocol` declares a typed message contract. `agent` declares a
unit of state, concurrency, and failure that implements one or more
protocols. The `on Ping(msg) -> msg` handler is the compact form of:

```mty
on Ping(msg) {
  return msg
}
```

To see agents *running*, look at
[`examples/19_backend_service.mty`](https://github.com/hassard0/Mighty/blob/main/examples/19_backend_service.mty)
— it wires Echoer into a supervisor, sends it traffic, and runs
under a CPU budget.

For the full agent walkthrough see
[tour chapter 6](tour/06-agents.md) and
[tour chapter 7](tour/07-send-ask.md).

## 7. Your first test

Create `tests/echo_test.mty`:

```mty
import echo.{Echoer, Echo}

fn test_echo_replies() {
  let e = spawn Echoer()
  let r = e?Ping("hi") @1s
  assert_eq(r, Ok("hi"))
}
```

Then:

```bash
mty test
```

Output (cargo-test-style):

```
running 1 test
test test_echo_replies ... ok

test result: ok. 1 passed; 0 failed; finished in 0.01s
```

`mty test` walks `tests/` in the current package, runs every
`fn test_*` it finds, and prints a `cargo test`-style report. Exit
code: 0 on all-pass, 1 on any failure. Pass `--dir <path>` to test a
directory other than `tests/`. See
[reference/stdlib/test.md](reference/stdlib/test.md) for the full
discovery + execution model.

(In v0.2 this was a separate `mty-test` binary; the v0.3 release
folded it into the main `mty` CLI as `mty test`.)

## 8. Format

```bash
mty fmt src/main.mty
```

The formatter is a stable per-node rewriter (slice 2). Pass
`--check` to verify formatting without writing, or `--stdin` to
read from standard input.

## 9. Inspect intermediate forms

```bash
mty dump --cst src/main.mty
mty dump --ast src/main.mty
mty dump --hir src/main.mty
mty dump --sir src/main.mty   # post-typeck; only valid if check passes
```

Use these to debug parser / lowering behavior or to write your own
tooling against the compiler. See
[reference/cli/mty-dump.md](reference/cli/mty-dump.md).

## 10. Explain a diagnostic

The diagnostic-code registry is stable: `MTxxxx` codes are
permanent, and the explain text is shipped inside the binary. After
hitting any error, run:

```bash
mty explain MT3001
```

to see the full Cause / Example / Fix / Spec block for that code.
Fifteen of the most-hit codes have been polished to that level of
detail; the rest are 2–4 sentence paragraphs.

The historical `SD####` prefix (pre-v0.7) is still recognised as an
alias — `mty explain SD0001` is equivalent to `mty explain MT0001`.

## What's next

- Work through the [tour](tour/README.md) chapter by chapter — it
  walks the 20 canonical examples one feature at a time.
- Read the [language specification](spec/v1.0-rc.md) (v1.0-RC2)
  when you want the normative answer.
- Browse the [CLI reference](reference/cli/mty.md) for every flag.
- Skim the [FAQ](faq.md) for the most common questions and the
  installation gotchas.
- Open an issue on
  [github.com/hassard0/Mighty](https://github.com/hassard0/Mighty)
  if anything surprises you. Bug reports are very welcome.
