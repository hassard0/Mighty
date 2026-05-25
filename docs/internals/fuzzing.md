# Fuzzing

Mighty ships four `cargo-fuzz` targets that exercise the front-end and
the Cranelift back-end against arbitrary input. They were introduced in
v0.9 as part of the pre-1.0 bug-bash. This page documents what each
target asserts, how to run them, how to reproduce a finding, and the
planned CI integration.

## Targets

| Crate                          | Target           | Property                                                                                                                              |
| ------------------------------ | ---------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `mty-syntax`                   | `parser_fuzz`    | `parse(s)` never panics on any UTF-8 input. Errors are recorded in `ParseResult.errors`; crashes are bugs.                            |
| `mty-types`                    | `typeck_fuzz`    | parse + HIR lower + `check_package_typed` never panics. This exercises name resolution, HM inference, bidirectional check, effects.   |
| `mty-fmt`                      | `fmt_idempotence`| `format(format(x)) == format(x)` for any `x` (idempotence + no panic in the formatter or trivia engine).                              |
| `mty-codegen-cranelift`        | `codegen_fuzz`   | When the front-end accepts a program cleanly, lowering to Cranelift IR + emitting an object file never panics.                        |

Each target lives in `crates/<crate>/fuzz/fuzz_targets/<target>.rs`
with a sibling `Cargo.toml` and `corpus/<target>/` seed directory.
`cargo fuzz init` creates these as standalone packages (each has
`[workspace]` in its manifest to opt out of the root workspace), so
they don't pull in the heavyweight workspace build for normal
`cargo test`.

## Seed corpus

Each target's `corpus/<target>/` starts with:

- All 20 `examples/*.mty` files (`example_*.mty`)
- 5 self-host source files (`selfhost_*.mty`)
- `empty.mty` (zero bytes)
- `minimal_main.mty` (`fn main() {}\n`)

libFuzzer extends the corpus as it discovers new coverage. Don't check
the extended corpus into git — the seeds are enough to bootstrap a
fresh box, and the rest is host-specific.

## Running the smoke fuzz

Prerequisite: Rust nightly (libFuzzer needs `-Z sanitizer` and friends)
and the `cargo-fuzz` subcommand:

```bash
rustup toolchain install nightly
cargo install cargo-fuzz
```

Then, from any crate that owns a fuzz directory:

```bash
cd crates/mty-syntax
cargo +nightly fuzz run parser_fuzz -- -max_total_time=300
```

The `-max_total_time=300` caps the run at 5 minutes (the v0.9 smoke
budget). Drop it for an unbounded run.

The first build is slow (it has to compile the whole front-end with
`-Cinstrument-coverage` and libFuzzer wired in). Subsequent runs are
fast.

## Reproducing a fuzz finding

When libFuzzer hits a crash it writes the input to
`fuzz/artifacts/<target>/crash-<hex>`. Replay it deterministically
with:

```bash
cargo +nightly fuzz run parser_fuzz fuzz/artifacts/parser_fuzz/crash-<hex>
```

For a minimised reproduction:

```bash
cargo +nightly fuzz tmin parser_fuzz fuzz/artifacts/parser_fuzz/crash-<hex>
```

Triage rule of thumb (v0.9):

- **Panic / abort / segfault** → v1.0 blocker. File in
  `FUZZ_V0_9_NOTES.md` with input excerpt + which crate panicked. Fix
  in the smallest scope possible.
- **Stack overflow on deeply-nested input** → ship with a depth limit
  in the parser (v0.9.x) or document as a known limit (v1.x).
- **Slow input (>1s)** → log but don't block; revisit when we benchmark.

## Adding a new target

1. From the crate root:
   ```bash
   cd crates/<crate>
   cargo +nightly fuzz init   # only on first target in the crate
   cargo +nightly fuzz add <name>
   ```
2. The auto-generated `fuzz/Cargo.toml` needs an explicit `[workspace]`
   line (just `[workspace]` on its own — empty table). Without it,
   cargo gets confused about workspace membership because `fuzz/` is
   physically inside a workspace member.
3. Add any extra path-deps your target needs (typically `mty-driver`
   for end-to-end pipelines).
4. Write a `#[no_main]` target in `fuzz_targets/<name>.rs` using
   `libfuzzer_sys::fuzz_target!`. Convert raw bytes to UTF-8 with
   `std::str::from_utf8` and bail on `Err` — most of our inputs are
   source code.
5. Seed `fuzz/corpus/<name>/` with the same example/self-host set the
   other targets use, plus a couple of minimal cases.
6. Build once locally to verify (`cargo +nightly fuzz build <name>`).
7. Document the new target in this file and in `FUZZ_V0_9_NOTES.md`.

## CI integration (planned)

Cargo-fuzz needs nightly Rust and only runs on Linux/macOS reliably in
CI. The plan:

- **Nightly job**: 5 minutes per target on Linux. Upload any new
  artifacts/crash inputs as build artifacts. Open a tracking issue on
  the first novel crash signature each week.
- **PR fast path**: build all four targets (`cargo +nightly fuzz build`)
  so the targets stay compileable — no fuzz time spent, just the
  type-check / link gate.
- **Release gate**: 30 minutes per target before a tag. If a new
  crash appears, block the release.

Until that infrastructure lands, contributors are encouraged to run the
smoke fuzz locally before tagging.

## Architecture

The four targets cover the pipeline at orthogonal stages:

```
            UTF-8 bytes
                │
                ▼
       ┌──────────────────┐
       │   mty-syntax     │  ← parser_fuzz
       │   (parse)        │
       └────────┬─────────┘
                │ GreenNode
                ├──────────────────────┐
                ▼                      ▼
       ┌──────────────────┐   ┌──────────────────┐
       │   mty-fmt        │   │   mty-hir lower  │
       │ (format×2)       │   │     + types      │  ← typeck_fuzz
       └──────────────────┘   └────────┬─────────┘
       ↑ fmt_idempotence              │ typed pkg
                                       ▼
                              ┌──────────────────┐
                              │   borrow check   │
                              │   IR lower       │
                              │   monomorphize   │
                              │   cranelift IR   │  ← codegen_fuzz
                              │   object emit    │
                              └──────────────────┘
```

A bug in an early stage tends to surface in the early target first —
e.g. a parser overflow is `parser_fuzz`, not `typeck_fuzz`. When
triaging, start at the leftmost target that crashes on a given input.
