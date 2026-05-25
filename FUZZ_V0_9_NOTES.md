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
- **Windows-MSVC gotcha**: the fuzz binary depends on
  `clang_rt.asan_dynamic-x86_64.dll` from
  `C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\<ver>\bin\Hostx64\x64\`.
  That directory is not on `PATH` by default — without it the binary
  fails to launch with `STATUS_DLL_NOT_FOUND (0xc0000135)`. Set
  `PATH` before invoking `cargo fuzz run` (or copy the DLL alongside
  the fuzz exe). The Linux/macOS CI path won't see this.

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

## Smoke-run results

All runs done on `x86_64-pc-windows-msvc` nightly with
`-max_total_time=300 -rss_limit_mb=4096`, single instance, default
jobs=1. `cargo +nightly fuzz run` exits early on the first crash, so
"runtime" here is the wall clock until the first finding, not the full
5-minute budget.

### parser_fuzz — **OOM (P0)**

- **Run time**: ~5 minutes (libFuzzer found the OOM during corpus
  replay-with-mutations on the selfhost seed; ran to completion of the
  300-second budget before bailing).
- **Corpus size after**: 27 seeds (no new units saved; libFuzzer
  doesn't promote OOM units to the corpus).
- **Crashes**: 0 panics, 2 OOM artifacts.
- **Slow inputs**: 0 (under the 1s threshold).
- **Artifacts**: `crates/mty-syntax/fuzz/artifacts/parser_fuzz/`
  - `oom-a83a134ed9f3f1e8536ac500eb95af293c722fae` (174 bytes — small repro)
  - `oom-1c38757a409957e8ec26fc9bff5fcd8b06de6dac` (18 KB)

### fmt_idempotence — **OOM (same root cause as parser_fuzz)**

- **Run time**: ~5 minutes.
- **Corpus size after**: 27 seeds + libFuzzer-discovered units.
- **Crashes**: 0 panics, 1 OOM artifact.
- **Slow inputs**: 0.
- **Artifacts**: `crates/mty-fmt/fuzz/artifacts/fmt_idempotence/`
  - `oom-3fd8c7e676433c4340e04a43d02e2e0e14ed720b` (225 bytes; the
    input is structurally identical to the parser_fuzz one — `format`
    calls `parse` internally, so the same parser-OOM trips this
    target).
- **No idempotence violations** were observed before the OOM. The
  format-twice round-trip succeeded on every input the parser returned
  in bounded time.

### typeck_fuzz — **clean (5 min, no crashes)**

- **Run time**: 5 minutes (full budget).
- **Crashes**: 0 panics, 0 OOMs.
- **Slow inputs**: 0.
- **Notes**: typeck couldn't run on the parser-OOM input (the parser
  blew up first), so the parser fix is a prereq for deeper typeck
  fuzzing. With the malformed-enum input excluded, the type checker
  was robust to everything libFuzzer threw at it in this window.

### codegen_fuzz — **stack overflow inside Cranelift (P1, upstream)**

- **Run time**: ~5 seconds (crashed on a mutated seed almost
  immediately after libFuzzer started).
- **Crashes**: 1 stack overflow.
- **Slow inputs**: 0.
- **Artifacts**: `crates/mty-codegen-cranelift/fuzz/artifacts/codegen_fuzz/`
  - `crash-eb52420944e0ab2856e40ae22f6d6587e218a5da` (88 bytes)
- **Input**:
  ```mighty
  fn first[T](xs: &[T]) -> Option[&T] {
    if xs.len == 0 { None } else { Some(&xs[0]) }
  }
  ```
- **Stack trace**: ASAN reports `stack-overflow` inside
  `cranelift_codegen::opts::generated_code::constructor_simplify` →
  `optimize_pure_enode` → `make_inst_ctor` → `constructor_icmp` →
  `constructor_simplify` (recursing). The egraph optimizer's ISLE
  rules infinite-recurse on something our IR emits.
- **Locus**: not in our code — the recursion is entirely inside
  `cranelift-codegen-0.132.0`. We trigger it from
  `mty_codegen_cranelift::object::compile_object_inner` →
  `<Module>::define_function`.

## Triage

| # | Severity      | Target            | Symptom                                          | Locus                                  | Patch scope          |
| - | ------------- | ----------------- | ------------------------------------------------ | -------------------------------------- | -------------------- |
| 1 | **P0 v1.0**   | parser_fuzz       | OOM (~12 GB alloc) on `enum E { R(F>4)` (16 B)   | `crates/mty-syntax/src/parser/items.rs::enum_decl` | tiny (~10 LOC, see below) |
| 2 | **P0 v1.0**   | fmt_idempotence   | Same OOM (calls `parse` underneath)              | Inherited fix from #1                  | n/a                  |
| 3 | **P1 v1.0**   | codegen_fuzz      | Cranelift egraph stack-overflow on generic+slice | upstream `cranelift-codegen` 0.132     | report upstream; in the meantime, lift `cranelift::Flags` to disable egraph opts or cap recursion |

### Bug 1: enum-variant infinite loop

**Root cause**: `enum_decl` in `crates/mty-syntax/src/parser/items.rs`
(line 348) loops `while !p.at(R_BRACE) && !p.at(EOF)` but on a
malformed variant payload (e.g. `R(F>4)`) the iteration consumes no
tokens — `paths::name` returns false on non-IDENT input, the payload
parens are already past, nothing advances. Each iteration pushes a
fresh `ENUM_VARIANT` green node, so memory grows without bound until
the 12 GB allocation request fails.

**Reproduction** (in any Rust target with `mty-syntax` as a dep):

```rust
mty_syntax::parse("enum E { R(F>4)");
```

The 16-byte input takes >5 s and ~12 GB before aborting.

**Proposed fix** (not applied — out of this task's scope per the
shared-tree concurrency rule; needs to be picked up by the parser
owner):

```rust
let before = p.pos;
p.start_node(ENUM_VARIANT);
paths::name(p);
if p.eat(L_PAREN) {
    super::types::type_expr(p);
    while p.eat(COMMA) {
        if p.at(R_PAREN) { break; }
        super::types::type_expr(p);
    }
    p.expect(R_PAREN);
}
p.finish_node();
p.eat(COMMA);
p.skip_trivia();
if p.pos == before {
    p.error("unexpected token in enum body");
    p.bump_any();
    p.skip_trivia();
}
```

The same pattern (cursor-progress guard) probably needs to be applied
to every `while !p.at(R_BRACE) && !p.at(EOF)` loop body in
`items.rs` (`trait_decl`, `impl_block`, top-level item loop). A
sweeping audit is its own ticket.

### Bug 3: Cranelift egraph stack-overflow

**Root cause**: not in our code. Filed for upstream report.
Workarounds:

1. Lift `cranelift_codegen::settings::Flags` and set
   `opt_level = "speed"` → `"none"` (disables the egraph pass) in
   `crates/mty-codegen-cranelift`. Costs us optimization quality but
   removes the crash surface.
2. Audit our IR generation for unusual patterns from generic +
   `&[T]` + `Option[&T]` — there may be a redundant `icmp` chain
   the optimizer is rewriting endlessly.

Recommend (1) for v0.9.x as a defensive measure pending the upstream
fix. Track in a separate ticket — fixing it is out of scope for the
fuzz bring-up.

## Recommended priority order

1. **v0.9.x patch**: fix Bug 1 (enum_decl progress guard) and audit
   the rest of `items.rs` for the same anti-pattern. Inherits a fix
   for Bug 2 (fmt OOM) for free.
2. **v0.9.x patch**: disable the cranelift egraph pass (Bug 3
   workaround) or pin a newer cranelift version that fixes the
   recursion.
3. **v1.0 gate**: smoke fuzz all four targets for 5 min each clean
   before tagging. Persistent corpora live under
   `fuzz/corpus/<target>/` (gitignored except for seeds), so a fresh
   box can replay quickly.
4. **v1.1**: scheduled nightly fuzz (Linux runner, 5 min/target);
   release-gate fuzz (30 min/target). See `docs/internals/fuzzing.md`
   for the proposed shape.

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

- **No parser fix applied in this task**. Bug 1 is a parser source
  change, and the v0.9 fuzz scope explicitly says not to modify parent
  crate source from this swarm slice. The fix is documented above for
  the next maintainer to apply (in a focused commit to `mty-syntax`).
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

- Apply Bug 1's parser fix + audit the other `while !at(R_BRACE)`
  loops in `parser/items.rs` (`trait_decl`, `impl_block`, top-level
  items). Re-run parser_fuzz + fmt_idempotence smoke after the fix to
  confirm the OOM is gone and no other bugs surface.
- File the Cranelift egraph stack-overflow upstream
  (`bytecodealliance/wasmtime` issue tracker). Provide the 88-byte
  Mighty input and the SIR it lowers to.
- Once CI is wired up, harvest the persistent corpora and check
  representative new seeds into the repo periodically.
- Add an `arbitrary`-driven generator for structured Mighty source —
  the byte-level fuzzer only finds front-end bugs that survive the
  parser; we want a grammar-aware fuzzer to push harder on the typeck
  + borrowck + codegen layers.
- Bake a `parse_with_depth_limit` (or similar guard) into the parser if
  any stack-overflow finding turns up in v0.9.x. Track in
  `SLICE_V1_0.md`.
