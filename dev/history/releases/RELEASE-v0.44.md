# Mighty v0.44 Release Notes

**Tag:** `v0.44.0`
**Date:** 2026-06-01
**Status:** SHIPPED - agentic app reliability and release identity.

**Headline:** make Mighty easier for agents to build and debug by
closing trust gaps surfaced by Mighty IDE dogfooding.

## Summary

v0.44 starts by making the toolchain identify itself with the public
Mighty milestone, then removes a parser stack ceiling hit by large
agent-authored command routers. The theme for this release is simple:
generated app code should fail with useful diagnostics, not with stale
versions, silent defaults, or process-level stack overflows.

## Shipped

- **L9:** `mty --version` reports the Mighty language/toolchain
  milestone (`0.44.0`) instead of the internal Rust crate
  version `0.1.0`. The agent HTTP `/v1/agent/version` endpoint uses
  the same public version source.
- **L37:** `else if` ladders parse iteratively while preserving the
  nested `IF_EXPR` CST shape. A 512-arm ladder now passes parser
  regression coverage and `mty check`, removing the practical ceiling
  hit by Mighty IDE key/command dispatch growth.
- **L18:** host-backed `std.fs` calls now force the default `mty run`
  path onto the interpreter fallback instead of compiling through the
  Cranelift generic extern stub. The host dispatcher accepts
  agent-friendly aliases (`read_file`, `read_to_string`, `write_file`,
  `write_string`, and `read_dir`) so generated app code and docs can
  use the common file-operation names directly.

## Validation

- PR #18, #19, and #20 passed their required checks before merge.
- Main CI passed after rerunning two infrastructure-failed jobs that
  hit GitHub runner disk exhaustion.
- The `v0.44.0` Release workflow completed successfully and published
  platform binaries plus the conformance kit.
- `main` stayed protected; admin merge was used only to move green PRs
  through branch rules.

## Carry-forward priorities

- Continue expanding `std.fs` from interpreter-hosted calls toward a
  fully native capability ABI for AOT/JIT output.
- Broaden formatter rollout beyond safe top-level item shapes.
- Continue replacing scalar ABI pain points with structured result and
  command surfaces that agents can generate without manual id mirrors.
