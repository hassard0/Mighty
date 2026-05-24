# Stardust Conformance Suite

Per Stardust v0.1 spec §37. Each subdirectory holds tests for one category.

Slice-1 categories (populated): `lexical/`, `parser/`, `formatter_idempotence/`.
Other categories are placeholders; later slices fill them.

## Running

```
cargo test -p sdust-syntax --test parse_recovery
cargo test -p sdust-fmt --test idempotence
cargo test -p sdust-fmt --test round_trip
```

## Adding a test

1. Drop the input `.sd` file in the appropriate category.
2. Add a Rust test that loads it and asserts the expected outcome (parse OK, specific diagnostic, fmt idempotence, etc.).
