# parse_throughput

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
