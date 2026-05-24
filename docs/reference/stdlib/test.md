# `std.test`

Mighty-native test runner. Distinct from `cargo test` (which tests
the compiler); this runs `.sd` test files inside a Mighty package.

## Quick start

Put test files under `tests/` in your package:

```
my-package/
├── mighty.toml
├── src/
│   └── main.sd
└── tests/
    └── basic_test.sd
```

`tests/basic_test.sd`:

```sd
fn test_addition_works() {
  let x = 1 + 2
  // a non-trapping body counts as a passing test.
}

fn test_panics_fails() {
  panic("intentional")
}
```

Run with:

```bash
mty-test
# tests/basic_test.sd ... ok
# tests/basic_test.sd ... FAILED
#   reason: trap MT0901: intentional
# test result: 1 passed; 1 failed; 2 total
```

Exit code: 0 on all-pass, 1 on any failure.

## Discovery

- Walks `tests/` (or `--dir <path>`) recursively.
- Every `.sd` file is considered.
- Every top-level `fn` whose name begins with `test_` is a test.
- Files are visited in lexicographic order; tests within a file run
  in declaration order.

## Execution

Each test file is parsed → HIR-lowered → type-+borrow-checked →
MtyIR-lowered → invoked through the MtyIR interpreter
(`run_fn_with_budget`) with a 5,000,000-step budget per test.

A test passes if its body returns normally. A test fails if it traps
(via `panic` or any other trap), exhausts the step budget, or returns
a non-zero exit.

## Reporter

`cargo test`-shaped:

```
test basic_test::test_addition_works ... ok
test basic_test::test_panics_fails ... FAILED
  reason: trap MT0901: intentional

test result: 1 passed; 1 failed; 2 total
```

## Determinism

The runner uses the slice-6 MtyIR interpreter, which is fully
deterministic given the same input. v0.3 will add a `--seed <N>`
flag (spec §A39) for tests that exercise the deterministic-runtime
seed surface.

## CLI surface

```
mty-test [--dir <path>]
```

- `--dir <path>` overrides the default `tests/`.
- `--help` prints usage.

In v0.3 this merges into the main `mty` CLI as `mty test`.

## Known limits (v0.2)

- The `test_` name prefix is the only discovery rule. A proper
  `test fn name() { ... }` syntax + `#[test]` attribute is planned
  for v0.3 (requires a parser change that lives outside this slice's
  work area).
- No per-test deterministic-seed plumbing yet (A39).
- No parallel test execution (interpreter is single-threaded).
