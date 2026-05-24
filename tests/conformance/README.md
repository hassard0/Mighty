# Mighty Conformance Suite

Per Mighty v0.1 spec §37. Each subdirectory holds tests for one category.

Slice-1 categories (populated): `lexical/`, `parser/`, `formatter_idempotence/`.
v0.5 additions: `control_flow/` (break, continue, iterator protocol — see
`control_flow/README.md`). Other categories are placeholders; later
slices fill them.

## Running

```
cargo test -p mty-syntax --test parse_recovery
cargo test -p mty-fmt --test idempotence
cargo test -p mty-fmt --test round_trip
```

## Adding a test

1. Drop the input `.sd` file in the appropriate category.
2. Add a Rust test that loads it and asserts the expected outcome (parse OK, specific diagnostic, fmt idempotence, etc.).
