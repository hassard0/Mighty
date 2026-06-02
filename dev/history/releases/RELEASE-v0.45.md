# Mighty v0.45 Release Notes

**Tag:** `v0.45.0`
**Date:** 2026-06-02
**Status:** SHIPPED - shim-less file I/O for agent-built apps + real L28 fix.

**Headline:** v0.45 lands native `std.fs` on the JIT/AOT path so
agent-built CLIs no longer need the IDE's file-I/O shim, broadens the
formatter rollout to function and type declarations, ships a
structured `mty check --json` result, and corrects the v0.42 T1 / v0.44
attribution for the L28 Vec liveness bug — the actual codegen fix
landed in this release.

## Shipped

- **PR #22 — CI disk headroom + Windows serial test.** Cuts CI runner
  disk pressure by stripping debug info from test binaries
  (`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`) and
  serialises the Windows `cargo test` lane (`--test-threads=1`). This
  unblocks the workspace test crate from exhausting the Windows
  runner's `C:` drive partway through; combined with PR #27's real L28
  fix it lets every required platform stay green on full `cargo test
  --workspace` runs.
- **PR #23 — `mty check --json` structured result document.** Adds a
  stable JSON envelope to `mty check`, replacing the
  stringly-formatted check output for agent callers. 43 new tests
  cover happy-path, multi-diagnostic, and error-shape coverage; the
  envelope shape is pinned through `crates/mty-cli/tests/check_json`
  so future structured-result changes ship a typed diff.
- **PR #24 — `scripts/test-like-gha.sh` local CI-mimic + pre-push
  opt-in.** Future agents (and the developer machine) can now
  reproduce the GHA `CARGO_PROFILE_DEV_DEBUG=0 /
  CARGO_PROFILE_TEST_DEBUG=0` envs locally, so codegen bugs that only
  surface under stripped-debug profiles (exactly the class PR #27
  fixes) are caught before push instead of after CI.
- **PR #25 — Native `std.fs` JIT/AOT + surface broaden.** Closes the
  marquee v0.44 carry-forward (**L18 P1**): `std.fs.*` calls on the
  default `mty run` (Cranelift JIT) and `mty build` paths now reach
  the runtime directly instead of forcing the interpreter fallback.
  Adds 11 new runtime ABI symbols
  (`mty_runtime_fs_{read,read_to_string,read_dir,write,write_string,append,exists,metadata,create_dir_all,remove_file,remove_dir_all}`),
  matching the existing capability gate (`MT4001`) at typeck. The
  hosted dispatcher and aliasing from v0.44 stay so generated apps
  see the same surface from either backend. Mighty IDE's file-I/O
  shim becomes droppable as a follow-up.
- **PR #26 — Formatter rollout for `fn` / `struct` / `enum` / `type`
  decls.** Extends the syntax-aware item formatter beyond top-level
  `const` (v0.43 L26) to function, struct, enum, and type-alias
  declarations. Emit-identical against the 67-file `.mty` corpus, so
  the rollout reformats nothing it does not have a regression test
  for.
- **PR #27 — Actually fix L28 codegen (debug=0 SEGV).** Corrects the
  v0.42 T1 attribution: `Vec[T]` is an opaque prelude ADT registered
  as `Layout::scalar(PTR_BYTES)`, but `crate::aggregate::is_aggregate`
  was returning `true` for any `IrTy::Adt`, sending `let mut v = v0`
  and `let mut v = arg` through the aggregate-Copy memcpy path. That
  copied 8 bytes of `Vec`'s 32-byte runtime header into an 8-byte
  slot and re-bound the dest Variable to the truncated slot, dangling
  the `cap`/`data`/`elem_size` fields when the helper returned. Under
  the default `[profile.dev]` (`opt-level=0 debug=2`) the OOB stores
  happened to land on Rust frame bytes that the JIT didn't touch
  before the readback; under PR #22's `CARGO_PROFILE_DEV_DEBUG=0` —
  which mirrors GHA — the bytes were overwritten and the JIT SEGV'd
  (`STATUS_ACCESS_VIOLATION`). Fix: `is_opaque_adt` short-circuit in
  `lower_assign`'s aggregate-Copy/Move branch, just `def_var` the
  pointer through, matching what the LLVM backend already does. Both
  v0.42 ignored Linux/macOS L28 branches unignored; two new
  pinpoint regressions guard the rebind path.

## Corrections

- **L28 was not actually fixed by v0.41 T3 / v0.42 T1.** Both releases
  documented an L28 closure that was, in reality, an artefact of the
  default `[profile.dev]` debug=2 metadata. The real codegen bug —
  opaque-ADT use-Copy memcpy truncation — survived to v0.45 and is
  closed only by PR #27. The v0.45 lessons doc has been updated to
  call this out so future work doesn't trust the prior attribution.
- **v0.44 release notes attributed v0.42's GHA failures to "disk
  exhaustion".** That was a misattribution. Some runs did hit disk
  pressure, but the underlying recurring red was the real L28 SEGV
  exposed by debug=0 test binaries. PR #22 (debug=0 + serial Windows)
  + PR #27 (the actual codegen fix) together close that loop, with
  PR #24 making the failure mode locally reproducible going forward.

## Validation

- All six PRs (#22, #23, #24, #25, #26, #27) passed required CI
  checks before merge.
- Main CI passed on the v0.45.0 merge commit across the six required
  gates: `test`, `test-minimal`, `msrv`, `clippy-strict`, `bench`,
  `security`.
- The `v0.45.0` Release workflow completed across the 5-platform
  matrix and published platform binaries plus the conformance kit.
  The conformance-kit double-upload trap patched in
  commit `2457b47` is no longer load-bearing — `release.yml` ships a
  single conformance-kit upload per tag.
- `main` stayed protected; admin merge was used only to move green
  PRs through branch rules, matching v0.44's policy.

## Carry-forward priorities

- Rework `examples/34_taint_untaint.mty` so the `std.fs.write`
  destinations are per-run tempdirs, then re-bump the
  `examples_passing_floor_holds` floor to 28.
- Continue broadening the formatter rollout — `impl` blocks,
  agents, protocols, and trait declarations are the next safe
  layers.
- Drop the Mighty IDE's file-I/O shim now that `std.fs` works
  natively under `mty build`/`mty run`.
- Continue the structured-result push past `mty check --json` into
  the remaining scalar-ABI command paths (`mty fmt`, `mty find`,
  `mty fix`).
