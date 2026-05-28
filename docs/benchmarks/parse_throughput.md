# parse_throughput

> **Baseline from Mighty v0.6 (recorded 2026-05-24).** These numbers
> have not been refreshed against v0.31. To run current measurements,
> see [`benches/README.md`](https://github.com/hassard0/Mighty/blob/main/benches/README.md) and the
> per-impl build steps in
> [`benches/parse_throughput/README.md`](https://github.com/hassard0/Mighty/blob/main/benches/parse_throughput/README.md).

**Workload:** lex + parse a 10 002-line synthetic Mighty source
(~130 KB; deterministic — see `crates/mty-bench/src/fixtures.rs`).

**Spec alignment:** §0 tooling baseline. The LSP, formatter, and
incremental compiler all sit on top of this pipeline.

## Numbers

| Impl | Median | p95 | p99 | Notes |
|---|---|---|---|---|
| Mighty v0.6 (full pipeline) | 6.19 ms | 10.01 ms | 10.40 ms | parse_source: lex + CST + diag collection |
| Mighty v0.6 (lex only) | (see criterion HTML) | | | logos pass only |
| Rust + logos 0.14 | (pending — see methodology.md) | | | hand-written tokenizer using the same crate |
| Go bufio scanner | (pending — Reference env) | | | hand-written scanner; no AST |
| C++ hand-written | (pending — Reference env) | | | -O3 single-pass tokenizer |

(`pending` cells will be filled in by the next run on a host with the
relevant toolchain. The comparator impls are in
`benches/parse_throughput/<lang>/` and ready to invoke.)

### Recorded values (this host, 2026-05-24)

```
parse_throughput       median=     6.192 ms  p95=    10.011 ms  p99=    10.401 ms
```

Source size: 132 842 bytes ⇒ **~21 MB/s** at the median.

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
