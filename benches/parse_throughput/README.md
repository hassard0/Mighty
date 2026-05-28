# `parse_throughput/` — lex + parse a 10 002-line synthetic source

## What we measure

End-to-end **lex + parse** of a deterministic synthetic Mighty source
(`synth_source(units=10_000)`) — roughly 10 002 lines / ~130 KB. We
time the operation over many iterations and report
median / p95 / p99 milliseconds, plus a derived bytes/sec at the
median.

This is the foundation the LSP, formatter, and incremental compiler
all sit on top of. Spec §0 tooling baseline.

| Knob | Value |
|---|---|
| Source size | ~10 002 lines / ~130 KB synthetic |
| Iterations (CLI runner) | 30 |
| Output | bytes/sec at the median |

## Layout

```
parse_throughput/
├── mighty/   # README pointing to crates/mty-bench — the Mighty impl
│             # is driven by the criterion bench + mty-bench-runner,
│             # not a standalone binary in this directory.
├── rust/     # logos 0.14 lexer + hand-written recursive descent
│   ├── Cargo.toml
│   └── src/main.rs
├── go/       # bufio.Scanner with a custom split function
│   ├── go.mod
│   └── main.go
└── cpp/      # hand-written single-pass scanner (lexer only)
    ├── Makefile
    └── main.cpp
```

**Note on `mighty/`:** unlike the other categories, the Mighty
implementation here is *not* a standalone binary in this directory.
It runs through `crates/mty-bench`'s fixture generator (which mirrors
the synth source the other comparators build inline). See
[`mighty/README.md`](mighty/README.md) for the entry points.

Each language comparator is otherwise standalone — its own
`Cargo.toml` / `go.mod` / `Makefile`, no shared workspace.

## Building and running

**Mighty (via `crates/mty-bench`):**

```bash
cargo bench -p mty-bench --bench parse_throughput
./target/release/mty-bench-runner --category parse-throughput --iters 30
```

**Rust (logos):**

```bash
cd rust && cargo run --release -- 30
```

**Go (bufio):**

```bash
cd go && go run main.go --iters 30
```

**C++ (hand-written):**

```bash
cd cpp && make run
```

Or, from the repo root, run all available toolchains at once:

```bash
./benches/run.sh   # auto-detects rust / go / c++ (and stardust if built)
```

## Output shape

Each impl prints a single line of the form:

```
<lang>_parse_throughput: median=X.XXX ms  p95=X.XXX ms  p99=X.XXX ms
```

stable enough for the doc-rendering scripts to parse.

## What NOT to expect apples-to-apples

- The Rust / Go / C++ comparators are **lexer-only**. They don't
  build an AST or collect diagnostics. The Mighty number is
  **full pipeline** (lex + CST build + diagnostic collection) — a
  ~3-5x penalty over lex-only is expected and intentional.
- These are **research-grade comparators**, not production
  benchmarks. The synthetic fixture exercises only a small subset of
  Mighty's grammar.
- The numbers in [`docs/benchmarks/parse_throughput.md`](../../docs/benchmarks/parse_throughput.md)
  are a **v0.6 baseline** and have not been refreshed against current
  Mighty. Run the suite locally if you want trustworthy current
  numbers.

## Result page

Rendered numbers + interpretation:
[`../../docs/benchmarks/parse_throughput.md`](../../docs/benchmarks/parse_throughput.md).
