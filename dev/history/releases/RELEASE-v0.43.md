# Mighty v0.43 - Draft Release Notes

**Tag:** `v0.43.0`
**Date:** TBD
**Status:** DRAFT - IDE-dogfooding correctness rollup.

**Headline:** make the language more predictable for agents building
apps by closing high-signal Mighty IDE lessons in parsing, lowering,
interpreter mutation, formatting, and native build diagnostics.

## Summary

v0.43 is the follow-on release to v0.42's IDE-blocker closure. It
focuses on cases that make agent-authored apps brittle: eager logical
operators, interpreter/native disagreement for mutating methods,
parser precedence surprises, no-op formatting, and native build output
that previously blurred missing-linker fallback with real linker
failure.

## Release candidates

- **L47:** `&&` and `||` lower through short-circuit control flow, so
  guard-then-use expressions do not evaluate the protected RHS.
- **L12 (P0):** interpreter statement-form `Vec`/`String` mutators now
  write back to addressable receivers, matching native behavior for
  `v.push(x)`, `v.pop()`, and `v.clear()`.
- **L26 follow-up:** `mty fmt` starts syntax-aware formatting for
  comment-free top-level `const` declarations, including declaration
  spacing, generic type args, simple initializer expressions, and
  optional semicolons.
- **L46:** prefix operators now let postfix calls and indexes bind to
  their operand, so `!pred(x)` parses as `!(pred(x))` while invalid
  calls of non-callable unary values remain rejected.
- **L10:** native builds now distinguish missing linker fallback from a
  linker that ran and failed. Missing linkers still produce
  object-only success; real linker failures return an error with the
  emitted object path and linker stderr.

## Validation plan

- Keep the full CI matrix green before tagging: Ubuntu, macOS,
  Windows, minimal features, strict clippy, MSRV, build smoke,
  `mty-bench`, and `cargo audit`.
- Run focused local coverage for parser, IR lowering, interpreter,
  formatter, CLI conformance, driver native build behavior, and
  Cranelift Vec liveness before cutting the tag.
- Keep `main` protected. Merge only through reviewed, green PRs; do
  not weaken branch protection to move the release forward.

## Carry-forward priorities

- **L18 (P1):** expose `std.fs` as a Mighty-callable capability API.
- Broaden `mty fmt` beyond top-level `const` once the existing `.mty`
  corpus has an agreed reformat path.
- Publish #253 SWE-bench numbers and finish #262 BOLT training-profile
  path.
