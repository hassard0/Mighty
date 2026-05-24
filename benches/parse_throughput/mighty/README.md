# Mighty parse_throughput impl

The Mighty impl lives in
`crates/mty-bench/benches/parse_throughput.rs` (criterion) and
`crates/mty-bench/src/bin/mty-bench-runner.rs` (CLI summary).

Run via:

```bash
cargo bench -p mty-bench --bench parse_throughput
./target/release/mty-bench-runner --category parse-throughput --iters 30
```

Mighty uses `logos 0.14` for its lexer (see
`crates/mty-syntax/src/lexer.rs`) — same dependency the Rust
comparator uses — so the comparison is *Mighty's pipeline (lex + CST
build + parse)* vs *raw Logos lexer*. Any gap is attributable to:

- CST node allocation (rowan green tree)
- Parser combinator overhead
- Diagnostic collection

See `docs/benchmarks/parse_throughput.md` for the recorded numbers.
