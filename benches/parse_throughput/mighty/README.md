# Stardust parse_throughput impl

The Stardust impl lives in
`crates/sdust-bench/benches/parse_throughput.rs` (criterion) and
`crates/sdust-bench/src/bin/sdust-bench-runner.rs` (CLI summary).

Run via:

```bash
cargo bench -p sdust-bench --bench parse_throughput
./target/release/sdust-bench-runner --category parse-throughput --iters 30
```

Stardust uses `logos 0.14` for its lexer (see
`crates/sdust-syntax/src/lexer.rs`) — same dependency the Rust
comparator uses — so the comparison is *Stardust's pipeline (lex + CST
build + parse)* vs *raw Logos lexer*. Any gap is attributable to:

- CST node allocation (rowan green tree)
- Parser combinator overhead
- Diagnostic collection

See `docs/benchmarks/parse_throughput.md` for the recorded numbers.
