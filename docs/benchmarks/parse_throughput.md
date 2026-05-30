# parse_throughput

> **Last refreshed: v0.38 (2026-05-29) on vulcan** (Dell, Intel Xeon,
> 16 cores, Ubuntu 24.04, Rust 1.95.0, PGO release-pgo profile).
> Mighty + Rust-logos + C++ hand-written comparator numbers are
> v0.38 (rerun on this host); Go comparator retains the v0.6 baseline
> (Go toolchain not installed on vulcan — tracked as a v0.39 follow-up).

**Workload:** lex + parse a 10 002-line synthetic Mighty source
(~130 KB; deterministic — see `crates/mty-bench/src/fixtures.rs`).

**Spec alignment:** §0 tooling baseline. The LSP, formatter, and
incremental compiler all sit on top of this pipeline.

## Numbers

| Impl | Median | p95 | p99 | Notes |
|---|---|---|---|---|
| Mighty v0.38 (full pipeline) | 5.57 ms | 8.33 ms | 11.42 ms | parse_source: lex + CST + diag collection |
| Mighty v0.38 (lex only) | (see criterion HTML) | | | logos pass only |
| Rust + logos 0.14 | 0.44 ms | 0.46 ms | 0.49 ms | hand-written tokenizer using the same crate |
| C++ hand-written -O3 | 0.72 ms | 0.82 ms | 0.82 ms | g++ 13.3.0 single-pass tokenizer |
| Go bufio scanner | (pending — Go not installed on vulcan) | | | hand-written scanner; no AST |

(The Go comparator impl is in `benches/parse_throughput/go/` and ready
to invoke once a Go toolchain is wired into the vulcan bench host —
tracked as a v0.39 follow-up.)

### Recorded values (vulcan, 2026-05-29, v0.38)

```
parse_throughput       median=     5.575 ms  p95=     8.335 ms  p99=    11.417 ms
rust_parse_throughput  median=     0.443 ms  p95=     0.458 ms  p99=     0.492 ms  (bytes=132799)
cpp_parse_throughput   median=     0.724 ms  p95=     0.819 ms  p99=     0.820 ms  (bytes=132798)
```

Source size: 132 840 bytes ⇒ **~23.8 MB/s** at the median for the
full Mighty pipeline. The Rust + logos lex-only number lands at ~13×
the Mighty full-pipeline throughput — which is the right shape
(Mighty also builds a rowan CST and collects diagnostics; the
comparator just streams tokens). The C++ hand-written -O3 single-pass
tokenizer lands ~7.7× faster than the full Mighty pipeline (and ~1.6×
slower than the Rust+logos comparator — the C++ impl uses a simpler
state machine without logos's perfect-hash optimisation).

Holds within noise of the v0.33 refresh (5.637 → 5.575 ms median,
a ~1% improvement). The release-pgo profile's fat LTO + 1 codegen
unit doesn't move the parse needle much; this category is dominated
by CST allocation cost, not branch-prediction wins.

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
