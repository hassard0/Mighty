# formatter_idempotence/

v0.20-populated. Cases pinning the canonical formatter's idempotence
contract (spec §27.4, `crates/mty-fmt`).

The contract under test:

1. **Parse stability** — `parse(input.mty)` MUST succeed.
2. **Canonical fixpoint** — `fmt(input.mty) == canonical.mty`.
3. **Idempotence** — `fmt(fmt(input.mty)) == fmt(input.mty)` — the
   second `fmt` run is a no-op (already in canonical form).

Each case ships two source files:

```
NN_case_name/
  input.mty           — the source under test (often with unusual
                        whitespace, redundant blank lines, etc.)
  canonical.mty       — the canonical form `fmt(input.mty)` MUST
                        produce; idempotent under repeated `fmt`
  command.txt         — `check` (parse + typecheck on input.mty;
                        the fmt-equivalence comparison lives in
                        `crates/mty-fmt/tests/conformance_idem.rs`)
  expected_diagnostics.txt — usually empty (positive case)
  expected_exit_code.txt   — 0
  README.md           — what the case proves
```

The conformance_full harness asserts the input parses + type-checks
clean (positive case). The byte-equivalence with `canonical.mty` is
asserted by the fmt-specific test runner.

## Cases

| Case | Property under test |
|------|---------------------|
| `01_canonical_struct` | unusual struct whitespace -> canonical |
| `02_canonical_match` | match arm spacing -> canonical |
| `03_canonical_effect_clause` | multi-row-var `!{| E, F}` -> canonical |
| `04_canonical_comments` | line + block comments preserved |
| `05_canonical_macro` | macro bodies preserved verbatim |
