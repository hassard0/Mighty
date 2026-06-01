# Mighty v0.44 - Draft Release Notes

**Tag:** `v0.44.0`
**Date:** TBD
**Status:** DRAFT - agentic app reliability and release identity.

**Headline:** make Mighty easier for agents to build and debug by
closing trust gaps surfaced by Mighty IDE dogfooding.

## Summary

v0.44 starts by making the toolchain identify itself with the public
Mighty milestone, then removes a parser stack ceiling hit by large
agent-authored command routers. The theme for this release is simple:
generated app code should fail with useful diagnostics, not with stale
versions, silent defaults, or process-level stack overflows.

## Release candidates

- **L9:** `mty --version` reports the Mighty language/toolchain
  milestone (`0.44.0-dev` on main) instead of the internal Rust crate
  version `0.1.0`. The agent HTTP `/v1/agent/version` endpoint uses
  the same public version source.
- **L37:** `else if` ladders parse iteratively while preserving the
  nested `IF_EXPR` CST shape. A 512-arm ladder now passes parser
  regression coverage and `mty check`, removing the practical ceiling
  hit by Mighty IDE key/command dispatch growth.

## Validation plan

- Keep the full CI matrix green before tagging: Ubuntu, macOS,
  Windows, minimal features, strict clippy, MSRV, build smoke,
  `mty-bench`, and `cargo audit`.
- Run focused parser, CLI, and driver checks for every IDE-dogfooding
  fix included in the release.
- Keep `main` protected. Use admin merge only to move green PRs through
  branch rules; do not weaken checks, force-push protection, or delete
  protection.

## Carry-forward priorities

- **L18 (P1):** expose `std.fs` as a Mighty-callable capability API so
  built Mighty apps can save/load without a Rust shim owning file I/O.
- Broaden formatter rollout beyond safe top-level item shapes.
- Continue replacing scalar ABI pain points with structured result and
  command surfaces that agents can generate without manual id mirrors.
