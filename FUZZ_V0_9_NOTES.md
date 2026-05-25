# Fuzz harness + bug bash — v0.9

This document captures the v0.9 fuzz-bring-up: tools installed, targets
shipped, smoke-run results (5 min per target), and triage. See
`docs/internals/fuzzing.md` for the steady-state docs (how to add new
targets, how to reproduce crashes, planned CI integration).

## Setup

- Rust nightly: `rustup toolchain install nightly` → installed
  `1.98.0-nightly (23a3312d9 2026-05-23)` on
  `x86_64-pc-windows-msvc`.
- `cargo install cargo-fuzz` → installed `cargo-fuzz v0.13.1`.
- `libfuzzer-sys 0.4.12` builds cleanly on Windows MSVC nightly via the
  standard `-Cinstrument-coverage` + libFuzzer toolchain. **No
  proptest fallback needed.**

Workspace integration: every fuzz subcrate adds an explicit
`[workspace]` line to its `Cargo.toml` so that cargo treats it as a
standalone package and the root workspace stays untouched. This was a
hard requirement — without it, `cargo fuzz build` errors out with
"current package believes it's in a workspace when it's not."

## Targets shipped (4/4)

| Target                                                | Asserts                                                                   |
| ----------------------------------------------------- | ------------------------------------------------------------------------- |
| `crates/mty-syntax/fuzz` → `parser_fuzz`              | `parse(s)` never panics                                                   |
| `crates/mty-types/fuzz` → `typeck_fuzz`               | parse + HIR lower + `check_package_typed` never panics                    |
| `crates/mty-fmt/fuzz` → `fmt_idempotence`             | `format(format(x)) == format(x)` and no panic in fmt path                 |
| `crates/mty-codegen-cranelift/fuzz` → `codegen_fuzz`  | Cranelift lowering + object emission never panics on well-typed input     |

Each target has a 27-file seed corpus: 20 `examples/*.mty`, 5 selfhost
sources (`lexer.mty`, `parser.mty`, `lower.mty`, `infer.mty`,
`nodes.mty`), `empty.mty`, and `minimal_main.mty`
(`fn main() {}\n`).

## Smoke-run results (5 min per target)

> All runs done on `x86_64-pc-windows-msvc` nightly, `--max_total_time=300`,
> single instance, default jobs=1. Numbers are recorded *after* the run
> in the order targets ran.

### TARGET_RESULTS_PLACEHOLDER

## Triage

### TRIAGE_PLACEHOLDER

## Recommended priority order

### PRIORITY_PLACEHOLDER

## Suggested CI integration

Short version (full plan in `docs/internals/fuzzing.md`):

1. **PR fast path**: `cargo +nightly fuzz build` each of the four
   targets — keeps them compiling, costs nothing in fuzz time. Should
   already be a green light on every PR.
2. **Nightly job (Linux)**: 5 minutes per target. Any new
   `fuzz/artifacts/<target>/crash-*` artifact triggers a tracking
   issue.
3. **Release gate**: 30 minutes per target before each tag. Any new
   crash signature blocks the release until triaged.

GitHub Actions doesn't have a managed Rust-nightly + libFuzzer image,
so the nightly + release jobs will need a custom `actions/setup-rust`
step pinning a specific nightly toolchain (the same one we install
locally — `2026-05-23` at the time of writing) plus an idempotent
`cargo install cargo-fuzz`.

## Working-agreement notes / interpretation calls

- The codegen fuzz target only runs codegen if the front-end accepts
  the program cleanly (no errors from parse/typeck/borrowck). We deemed
  it out of scope to fuzz codegen on type-broken IR — the IR lowerer
  has loud `expect`s for the "type-broken program" case, and asking
  codegen to be robust to a broken-by-construction program would
  duplicate front-end fuzzing.
- We use `compile_object` (to a tempfile) rather than `build_jit`
  because JIT execution actually runs `main`, which would be a fuzz of
  the interpreter not the codegen. The fuzz target is intentionally
  about lowering + Cranelift IR + the object writer.
- The fmt target asserts idempotence on whatever the parser produces.
  The parser is error-tolerant — it always returns *some* green tree —
  so we deliberately do not gate on `errors.is_empty()`. The intent is
  exactly to surface "parser produced a weird tree, fmt diverged on the
  second pass" bugs.
- `cargo fuzz init` defaults the package name to the parent crate
  unchanged (so it always creates a `mty-syntax-fuzz` package even
  when run under `mty-types`). We renamed each fuzz package to match
  its host crate and removed the auto-generated `fuzz_target_1.rs`.

## Post-v0.9 follow-ups

- Once CI is wired up, harvest the persistent corpora and check
  representative new seeds into the repo periodically.
- Add an `arbitrary`-driven generator for structured Mighty source —
  the byte-level fuzzer only finds front-end bugs; we want a
  grammar-aware fuzzer to push harder on the typeck + borrowck +
  codegen layers.
- Bake a `parse_with_depth_limit` (or similar guard) into the parser if
  any stack-overflow finding turns up in v0.9.x. Track in
  `SLICE_V1_0.md`.
