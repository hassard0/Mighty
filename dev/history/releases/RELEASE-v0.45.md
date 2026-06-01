# Mighty v0.45 - Draft Release Notes

**Tag:** `v0.45.0`
**Date:** TBD
**Status:** DRAFT - agent-built app shipping ergonomics.

**Headline:** reduce the remaining shim code agents need when building
real apps in Mighty.

## Direction

v0.45 picks up the carry-forward work from v0.44: make host-backed
stdlib behavior available through native capability ABI paths, continue
the formatter rollout without destructive rewrites, and replace
stringly/scalar command plumbing with structured result surfaces that
agents can inspect, test, and regenerate safely.

## Candidate tracks

- **Native capability ABI:** move `std.fs` beyond interpreter fallback
  for JIT/AOT output while preserving capability checks and clear
  diagnostics.
- **Formatter rollout:** expand syntax-aware formatting from safe
  top-level `const` items into more item kinds with regression tests
  for comments and whitespace preservation.
- **Agent command surfaces:** prefer structured result values over
  sentinel strings and mirrored IDs in CLI, LSP, and runtime control
  paths.
- **Release hygiene:** keep README, changelog, release notes, and
  `mty --version` aligned before every tag.

## Validation plan

- Keep full CI green before every release tag.
- Add focused smoke tests for each native capability ABI expansion.
- Keep Mighty IDE lessons as the priority source for release-gate
  fixes.
