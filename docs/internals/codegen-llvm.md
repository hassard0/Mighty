# Internals — LLVM backend (scaffold, slice 8)

`sdust-codegen-llvm` is **a scaffold only** in v0.1. The slice-8
build host had no LLVM / `llvm-config` installed, so the slice
leader degraded to Cranelift-only for the v0.1 native backend
(see [A46](../spec/v0.1-amendments.md#a46)).

The crate is wired into the workspace so:

- Future build hosts with LLVM 17 can enable it via
  `cargo build --features llvm`.
- The eventual lowering work has a place to land.

## Current behaviour

```rust
pub fn compile(_prog: &Program) -> Result<(), LlvmError> {
    #[cfg(not(feature = "llvm"))] Err(LlvmError::FeatureDisabled)
    #[cfg(feature = "llvm")] Err(LlvmError::NotYetImplemented)
}

pub const fn enabled() -> bool { cfg!(feature = "llvm") }
```

`LlvmError` has two variants: `FeatureDisabled` (the feature is off)
and `NotYetImplemented` (the feature is on but lowering hasn't been
written yet). Both are returned without panic so the driver can
fall back to Cranelift cleanly.

## Why scaffolded, not real

`inkwell 0.4` requires `llvm-config --version` to succeed at build
time and locate matching LLVM 17 libs. On a host without LLVM, the
build fails before any Rust code is compiled — that would have
broken the entire workspace, including the parts that have nothing
to do with codegen. Gating behind a feature flag preserves the
"workspace builds cleanly out of the box" property while leaving a
clear extension point.

## When to enable it

Once Stardust v0.2 begins the native-optimization push, the LLVM
backend becomes attractive because:

- LLVM's IR is far richer than Cranelift IR (slice 8 does
  hand-rolled bounds-check elision; LLVM does it for free)
- LLVM has GlobalISel + the legacy backends → way more target
  triples than Cranelift currently supports
- ThinLTO / PGO live in the LLVM ecosystem

The expected sequence:

1. v0.2 starts. Install LLVM 17 on the build host.
2. Toggle `default = ["llvm"]` in `sdust-codegen-llvm/Cargo.toml`.
3. Port the `lower.rs` shape from Cranelift to inkwell. The SIR
   surface is the same; mostly mechanical.
4. Add LLVM-specific optimization passes (inlining, mem2reg, GVN,
   DCE, loop unrolling).
5. Make Cranelift the `--debug` backend (it's faster to JIT) and
   LLVM the `--release` backend (it produces better code).

## Future LLVM ABI bridge

The runtime ABI (`stardust_runtime_*` symbols) is identical for
LLVM and Cranelift — they both link against the same C-ABI fns.
The `runtime_imports::RUNTIME_IMPORTS` table is the shared schema.

## Why we don't `cfg`-gate the *crate*

Workspace tooling (cargo, IDEs, clippy) handles feature flags
better than out-of-tree crates. Keeping `sdust-codegen-llvm` always
in the workspace means `cargo check --workspace` still type-checks
the LLVM scaffold's public API, even when the feature is off — so
nobody can land a refactor that breaks it without noticing.
