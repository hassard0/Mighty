# egraph stack-overflow on simple generic slice function

**Filed against**: `bytecodealliance/wasmtime` (cranelift-codegen
0.132)

**Mighty version**: v0.9.0 (commit `06b6efe`).

## Summary

`cranelift-codegen 0.132`'s egraph optimization pass infinite-
recurses on a single 88-byte Mighty input that lowers to perfectly
ordinary CLIF — a generic-over-`T`, `&[T]` → `Option<&T>` helper.
The recursion blows the C stack inside libFuzzer's instrumented
binary in well under a second. The recursion lives entirely inside
`cranelift_codegen::opts::generated_code::constructor_simplify`
(the ISLE-generated egraph entry point); nothing in the calling
backend (`mty-codegen-cranelift` → `cranelift_module` →
`Module::define_function`) is on the cycle.

Workaround: set `opt_level = "none"` in
`cranelift_codegen::settings::Flags` — disables the egraph pass at
the cost of optimization quality.

## Reproducer

### Mighty source

The full input is 88 bytes:

```mty
fn first[T](xs: &[T]) -> Option[&T] {
  if xs.len == 0 { None } else { Some(&xs[0]) }
}
```

The bug is triggered by the front-end accepting this program and
the IR lowerer emitting CLIF for the generic. We have not been
able to reproduce by hand-written CLIF — the issue depends on the
specific operand sequence the IR lowerer emits for a generic-over-
`T` slice indexing + Option-wrapping pattern.

### Cranelift configuration

```rust
let mut b = cranelift_codegen::settings::builder();
b.set("opt_level", "speed").unwrap();
b.set("is_pic", "false").unwrap();
let flags = cranelift_codegen::settings::Flags::new(b);
```

(default `mty-codegen-cranelift` flags; see
`crates/mty-codegen-cranelift/src/lower.rs::default_flags`.)

### Driver invocation

We trigger the crash via:

```rust
mty_codegen_cranelift::object::compile_object(&prog)
    .expect("compile-object");   // → SIGSEGV stack-overflow inside Cranelift
```

`prog` is the result of `parse → lower → typeck → ir-lower` of the
Mighty source above. We have not yet been able to extract a
self-contained CLIF reproducer because the IR lowerer is the only
producer of this CLIF shape — we'll attach a CLIF dump if you'd
like one (it requires us to add a debug-print pass).

## Stack trace (libFuzzer / ASAN, Windows MSVC)

Truncated to the recursive frames:

```
cranelift_codegen::opts::generated_code::constructor_simplify
cranelift_codegen::opts::generated_code::optimize_pure_enode
cranelift_codegen::opts::generated_code::make_inst_ctor
cranelift_codegen::opts::generated_code::constructor_icmp
cranelift_codegen::opts::generated_code::constructor_simplify   ← cycle
cranelift_codegen::opts::generated_code::optimize_pure_enode
cranelift_codegen::opts::generated_code::make_inst_ctor
cranelift_codegen::opts::generated_code::constructor_icmp
cranelift_codegen::opts::generated_code::constructor_simplify   ← cycle
...
[~5000 frames before the OS kills the thread]
```

ASAN tags the crash as `stack-overflow` (not a heap OOB).
Reproduces identically on:

- Windows 11 + nightly Rust `1.98.0-nightly (23a3312d9 2026-05-23)`
  + cargo-fuzz 0.13.1 + libFuzzer.
- (Not yet verified on Linux — the libFuzzer binary build path on
  Windows MSVC is the one that's instrumented; happy to repro on
  Linux if requested.)

## Suspected root cause

The recursion bounces between `constructor_simplify` and
`constructor_icmp` via `optimize_pure_enode` →
`make_inst_ctor`. That suggests an ISLE rewrite rule pair that
rewrites `icmp` ↔ something-that-resimplifies-to-`icmp` without
adding any new e-node, so the optimizer's saturation check doesn't
fire. The CLIF that triggers it is plausibly an icmp chain emitted
for the `if xs.len == 0` test plus the bounds check inside `xs[0]`
— specifically, a redundant comparison the optimizer keeps trying
to rewrite.

(Speculation — we have not bisected the ISLE rules. Happy to bisect
on request if you give us the right knob to enable rule-by-rule
emission.)

## Environment

| Field | Value |
|-------|-------|
| `cranelift-codegen` | `0.132.0` (workspace pin in
`Cargo.toml`) |
| Rust toolchain | `1.98.0-nightly (23a3312d9 2026-05-23)` |
| Host triple | `x86_64-pc-windows-msvc` |
| Optimizer level | `speed` (the default; `"none"` is a workaround) |
| Reproducer | `crates/mty-codegen-cranelift/fuzz/artifacts/codegen_fuzz/crash-eb52420944e0ab2856e40ae22f6d6587e218a5da` |

## Workaround we shipped

`crates/mty-codegen-cranelift/src/lower.rs::default_flags` documents
the `opt_level = "none"` escape hatch. The Mighty CLI exposes it
via a future `--no-opt` flag (v0.10 follow-up); meanwhile, programs
matching the generic-slice shape can be compiled by editing
`default_flags` to set `opt_level = "none"`.

We have NOT applied the workaround by default — the perf cost is
real, and the bug only affects one synthetic shape we have
identified so far. The escape hatch lives in code comments for
operators who hit a repro in the wild.

## What we'd like upstream to look at

1. **The cycle** — `constructor_simplify` ↔ `constructor_icmp` via
   `optimize_pure_enode` / `make_inst_ctor`. Adding a depth limit
   (or a more thorough saturation check) would close the crash
   surface even before the underlying rule pair is identified.
2. **The triggering CLIF shape** — we can produce a CLIF dump if
   the original Mighty input isn't useful. The pattern is generic-
   over-T + `&[T]` indexing + Option wrapping; we suspect any
   front-end that emits a similar icmp chain would hit it.
3. **Whether 0.133+ has fixed it** — we'd like to pin a newer
   cranelift once a fix lands.

## Cross-references

- Bug-bash notes: `FUZZ_V0_9_NOTES.md` (Bug 3) in the Mighty repo
  (`hassard0/Mighty`, commit `06b6efe`).
- Cleanup follow-up: `CLEANUP_V0_10_NOTES.md`.

Thanks for cranelift! Happy to provide additional repros, a CLIF
dump, or test against a candidate fix on request.
