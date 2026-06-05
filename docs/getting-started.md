# Getting Started

This page walks through installing the Mighty compiler, scaffolding
a package, running your first program, writing your first agent,
running your first test, and pointing you at the next read.

> **Pre-alpha status.** Mighty is at **v0.49.0** (toolchain)
> tracking spec **v1.0-RC5**. The language surface is feature-
> complete for v1.0 and frozen pending the eight open RFC comment
> windows. Pre-built binaries ship on every release; treat the
> language as unstable and please file issues for everything that
> surprises you.

## 1. Install

### Pre-built binaries

The fastest path. Releases page:
<https://github.com/hassard0/Mighty/releases>. Tarballs for Linux
x86_64, macOS arm64, and Windows x86_64.

```bash
# Linux / macOS — replace <v> with the latest tag (e.g. v0.49.0).
curl -L https://github.com/hassard0/Mighty/releases/download/<v>/mty-<v>-linux-x86_64.tar.gz | tar xz
sudo mv mty /usr/local/bin/
mty --version
```

### From source

For contributors or platforms without a binary release.

- **MSRV:** Rust 1.85.
- **Platforms:** Linux, macOS (Intel + Apple Silicon), Windows.
- **Dependencies:** a C linker (`clang` / `gcc` on \*nix, MSVC's
  `link.exe` on Windows) only if you plan to use `mty build` with
  the native target. `mty run` and `mty check` need only Rust.

```bash
git clone https://github.com/hassard0/Mighty
cd Mighty
cargo install --path crates/mty-cli
mty --version
```

If `cargo install` fails on Windows with a linker error, see the
[FAQ entry on the Windows DLL gotcha](faq.md#why-does-mty-fail-to-link-on-windows).
If your build fails on macOS with a `LC_BUILD_VERSION` warning,
see the [macOS note](faq.md#what-is-the-macos-lc_build_version-fix).

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

`mighty.toml` is the package manifest. The generated file is
minimal:

```toml
[package]
name = "hello"
version = "0.1.0"
edition = "2026"
profile = "host"

[deps]
```

See [reference/manifest.md](reference/manifest.md) for the full
manifest schema (workspaces, dependencies, `[wit]`, `[cluster]`,
build profiles).

`src/main.mty` is the entry point:

```mty
fn main() {
  log("hello, Mighty")
}
```

For canvas-driven web games, scaffold with the `web-game`
template:

```bash
mty new --template web-game my-game     # canvas-driven web game
```

The default `blank` template is what `mty new <name>` produces;
`web-game` is the only specialised template shipped today. The
`cli` and `agent` templates are on the v0.31 roadmap.

## 3. Check it

```bash
cd hello
mty check src/main.mty
```

`mty check` parses the source, builds the CST and HIR, runs the
type checker, the effect / capability / taint checker, and the
borrow checker, and prints any diagnostics. On success it prints
`ok: <path>` and exits 0.

```
ok: src/main.mty
```

If anything fails, the diagnostic prints with file, line, column,
and a stable diagnostic code (`MTxxxx`). Run `mty explain MTxxxx`
for a Cause/Example/Fix/Spec block. For example:

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
Spec:    §7.2 (unification) of v1.0-RC5.
```

## 4. Run it

```bash
mty run src/main.mty
```

`mty run` runs the full `check` pipeline, lowers the program to
MtyIR, JIT-compiles via Cranelift, and invokes `main`. Programs
whose MtyIR the native backend can't yet lower fall back to the
tree-walking interpreter transparently. The above program prints:

```
hello, Mighty
```

and exits 0. See
[reference/cli/mty-run.md](reference/cli/mty-run.md) for exit
codes, traps, the effect-handling model, and the `-- <argv>`
positional-passthrough.

## 5. Build it

```bash
mty build src/main.mty
# → wrote target/hello   (or hello.exe on Windows)

mty build --target wasm32-web src/main.mty
# → wrote target/hello.wasm   (Component Model component)

mty build --target wasm32-wasi src/main.mty
# → wrote target/hello.wasm   (WASI Preview 2)
```

`mty build` produces a real, runnable artefact. The native target
uses Cranelift to emit a host-format `.o`, then links via the
platform C linker. If no linker is on PATH, the `.o` is left in
`target/` and a helpful message tells you how to link manually
(see `MT8008`). If a linker is found but the link fails, `mty build`
exits with code 2 and prints the emitted object path plus the linker
error so agents and CI can distinguish a real native build failure
from an object-only fallback.

Override the linker with `$MTY_LINKER` (an absolute path or a bare
name on PATH; legacy `$STARDUST_LINKER` is still accepted). Force
the MSVC arg-rewrite path with `$MTY_LINKER_FLAVOR=msvc` when your
linker wrapper hides the basename. To attach native libraries to
every build in a package, add a `[build]` block to `mighty.toml`:

```toml
[build]
native-libs = ["m"]                       # → -lm (m.lib on MSVC)
link-search = ["/opt/whatever/lib"]       # → -L/...
frameworks  = ["Cocoa"]                   # macOS only
link-args   = ["--gc-sections"]           # raw passthrough
```

See `docs/reference/cli/mty-build.md` for the full env-var matrix
and the MSVC rewrite table.

The Wasm targets produce a Component-Model component (for the web
target) or a core Wasm module with Preview 2 imports (for WASI),
runnable under `wasmtime` / `wasmer` or any browser host.

See [reference/cli/mty-build.md](reference/cli/mty-build.md) for
the full flag list and current backend coverage matrix.

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
unit of state, concurrency, and failure that implements one or
more protocols. The `on Ping(msg) -> msg` handler is the compact
form of:

```mty
on Ping(msg) {
  return msg
}
```

To see agents *running*, look at
[`examples/19_backend_service.mty`](https://github.com/hassard0/Mighty/blob/main/examples/19_backend_service.mty)
— it wires `Echoer` into a supervisor, sends it traffic, and runs
under a CPU budget.

For the full agent walkthrough see
[tour chapter 6](tour/06-agents.md) and
[tour chapter 7](tour/07-send-ask.md). For LLM-driven agents jump
to [Demo 07](https://github.com/hassard0/Mighty/blob/main/demos/07_research_agent/README.md)
and the `@tool` example
[`examples/27_tool_attr.mty`](https://github.com/hassard0/Mighty/blob/main/examples/27_tool_attr.mty).

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
code: 0 on all-pass, 1 on any failure. Pass `--dir <path>` to
test a directory other than `tests/`. See
[reference/stdlib/test.md](reference/stdlib/test.md) for the full
discovery + execution model.

For LLM-agent regression testing with `*.eval.mty` suites that
compare against a panel of providers under byte-identical replay,
run `mty test --eval` — see
[`docs/internals/std-eval.md`](internals/std-eval.md).

## 8. Format

```bash
mty fmt src/main.mty
```

The formatter is a stable per-node rewriter (canonical
Wadler/Lindig pretty-printer). Pass `--check` to verify formatting
without writing, or `--stdin` to read from standard input.

## 9. Inspect intermediate forms

```bash
mty dump --cst src/main.mty
mty dump --ast src/main.mty
mty dump --hir src/main.mty
mty dump --sir src/main.mty   # post-typeck; only valid if check passes
```

Use these to debug parser / lowering behaviour or to write your
own tooling against the compiler. See
[reference/cli/mty-dump.md](reference/cli/mty-dump.md).

For live agent introspection, see `mty inspect` — it queries a
running runtime's control socket for agent snapshots, mailbox
backlogs, and (with `--cost`) per-LLM-call cost + latency from
the local SQLite observe store.

## 10. Explain a diagnostic

The diagnostic-code registry is stable: `MTxxxx` codes are
permanent, and the explain text is shipped inside the binary.
After hitting any error, run:

```bash
mty explain MT3001
```

to see the full Cause / Example / Fix / Spec block for that code.

The historical `SD####` prefix (pre-v0.7) is still recognised as
an alias — `mty explain SD0001` is equivalent to
`mty explain MT0001`.

## What's next

- Work through the [tour](tour/README.md) — pedagogical walk
  through the canonical examples 01–20.
- Skim the [examples index](https://github.com/hassard0/Mighty/blob/main/examples/README.md)
  — 36 one-file examples grouped by feature; the v0.27–v0.30
  LLM-agent surface starts at example 27.
- Read the nine [demos](https://github.com/hassard0/Mighty/blob/main/demos/README.md)
  — end-to-end apps showcasing real composed surfaces.
- Read the [language specification](spec/v1.0-rc.md) (v1.0-RC5)
  when you want the normative answer.
- Browse the [CLI reference](reference/cli/mty.md) for every flag.
- Skim the [FAQ](faq.md) for the most common questions and the
  installation gotchas.
- For the SWE-bench Verified marquee page see
  [`dev/history/benchmarks/swe-bench-smoke-v0.30.md`](https://github.com/hassard0/Mighty/blob/main/dev/history/benchmarks/swe-bench-smoke-v0.30.md).
- Open an issue on
  [github.com/hassard0/Mighty](https://github.com/hassard0/Mighty)
  if anything surprises you. Bug reports are very welcome.
