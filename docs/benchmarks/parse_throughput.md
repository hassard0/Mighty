# parse_throughput

> **Last refreshed: v0.33 (2026-05-28) on vulcan** (Dell, Intel Xeon,
> Ubuntu 24.04, Rust 1.95.0). Mighty + Rust-logos comparator numbers
> are v0.33; Go + C++ comparators retain the v0.6 baseline pending a
> comparator toolchain refresh on the benchmark host.

**Workload:** lex + parse a 10 002-line synthetic Mighty source
(~130 KB; deterministic — see `crates/mty-bench/src/fixtures.rs`).

**Spec alignment:** §0 tooling baseline. The LSP, formatter, and
incremental compiler all sit on top of this pipeline.

## Numbers

| Impl | Median | p95 | p99 | Notes |
|---|---|---|---|---|
| Mighty v0.33 (full pipeline) | 5.64 ms | 8.71 ms | 10.62 ms | parse_source: lex + CST + diag collection |
| Mighty v0.33 (lex only) | (see criterion HTML) | | | logos pass only |
| Rust + logos 0.14 | 0.18 ms | 0.19 ms | 0.22 ms | hand-written tokenizer using the same crate |
| Go bufio scanner | (pending — Reference env) | | | hand-written scanner; no AST |
| C++ hand-written | (pending — Reference env) | | | -O3 single-pass tokenizer |

(`pending` cells will be filled in by the next run on a host with the
relevant toolchain. The comparator impls are in
`benches/parse_throughput/<lang>/` and ready to invoke.)

### Recorded values (vulcan, 2026-05-28, v0.33)

```
parse_throughput       median=     5.637 ms  p95=     8.713 ms  p99=    10.617 ms
rust_parse_throughput  median=     0.183 ms  p95=     0.189 ms  p99=     0.220 ms  (bytes=132799)
```

Source size: 132 840 bytes ⇒ **~24 MB/s** at the median for the full
Mighty pipeline. The Rust + logos lex-only number lands at ~31× the
Mighty full-pipeline throughput — which is the right shape (Mighty
also builds a rowan CST and collects diagnostics; the comparator
just streams tokens).

### v0.6 baseline (Windows 11 dev laptop, 2026-05-24)

For continuity: Mighty v0.6 measured **median = 6.19 ms** for the
full pipeline (~21 MB/s) on a different host. Cross-host deltas are
shape, not absolute.

## Interpretation

The Mighty parse pipeline is logos-backed + a hand-written
recursive-descent parser that produces a rowan green-tree CST. The
**lex-only** path is a thin wrapper over logos and should land
within 10% of the Rust comparator's lexer-only number once we run it.

The **full pipeline** number is dominated by:

1. CST node allocation (rowan green tree).
2. Diagnostic collection (we collect every error, not just the first).
3. Parser state tracking (recursive descent with lookahead).

We expect a ~3-5x gap between Mighty's full pipeline and a bare
lexer in any language. That's the right shape — the IDE / LSP / fmt
all need the tree, not just tokens.

## v0.7+ optimisation targets

- **Token caching** between LSP and rustc-style incremental
  reparse. Today every parse rebuilds the green tree from scratch.
- **Arena green nodes** instead of `Arc`-heavy rowan defaults.
- **Single-pass diag throttle** (cap at N errors before bailing).

Tracked in: `BENCHMARKS_V0_6_NOTES.md` § Parse Throughput.

## v0.8 update

| Optimisation              | Status | Delta vs v0.6 baseline                                |
|---------------------------|--------|-------------------------------------------------------|
| Token cache (incremental) | DONE   | tokencache_edit ~107 MiB/s vs lex_full ~78 MiB/s on a midpoint edit; the real win is "re-lex 3 tokens" not "lex faster" |
| Diag throttle             | DONE   | `ParseOpts::max_diagnostics = 16` is ~25% faster on adversarial input; uncapped path unchanged |
| Arena green nodes         | DEFER  | Rowan green node arena change touches rowan upstream; not in v0.8 scope |

Microbench: `crates/mty-syntax/benches/lex_throughput.rs`.
Interpretation log: `BENCHMARKS_V0_8_NOTES.md`.
