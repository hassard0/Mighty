# Independent implementations of Mighty

> **Status**: v0.11 (2026-05-25). Python second-impl shipped as
> `impl-py/`. v1.0-RC2 freeze blocker #2 (external re-implementation)
> partially closed by this document.

## What counts as an "independent implementation" for v1.0?

An "independent implementation" of Mighty, for the purpose of v1.0
spec validation, satisfies all of the following:

1. **Spec-driven, not source-driven.** Built consulting only the
   normative spec documents:
   * `docs/spec/v1.0-rc.md`
   * `docs/spec/v0.1-amendments.md`
   * `docs/spec/CHANGELOG.md`
   * The `examples/*.mty` corpus as test fixtures
   The Rust reference implementation under `crates/` and
   `selfhost/` is OFF-LIMITS. Any consultation must be documented
   as a deliberate divergence point.

2. **Independent toolchain.** Different programming language, runtime,
   and parser strategy than the Rust reference. A pure-Python impl
   counts; a transliteration of `mty-syntax` to OCaml does NOT.

3. **Verifiable by tests.** Ships a test suite that covers at least
   the lexical and syntactic surface of the v1.0 example corpus and
   demonstrates zero-diagnostic round-trip on the official examples.

4. **Findings published.** Every spec ambiguity discovered during
   construction is logged. Ambiguities are the load-bearing
   v1.0-polish artefacts — they identify places where the spec
   doesn't actually pin behaviour.

A "full" independent implementation extends through HIR lowering,
type checking, borrow checking, and code generation. A "front-end"
independent implementation (the v1.0 freeze-blocker minimum) covers
the lexer and parser.

## Python impl status (`impl-py/`)

**Coverage**: front-end only — lexer + parser.

| Phase           | Status                                           |
|-----------------|--------------------------------------------------|
| Lexer           | Shipped. Every §3 token kind. 20/20 examples.    |
| Parser          | Shipped (subset). Every §4 item kind. 20/20 examples parse with zero diagnostics. |
| HIR lowering    | Out of scope (v0.12+)                            |
| Type checker    | Out of scope (v0.12+)                            |
| Borrow checker  | Out of scope (v0.12+)                            |
| Codegen         | Out of scope (v0.12+)                            |

**License**: MIT (matches the rest of the workspace).

**Test count at landing**: 135 tests passing in `python -m pytest
impl-py/tests/`.

**Findings count**: 16 documented spec ambiguities. See
[`PYTHON_IMPL_V0_11_NOTES.md`](../../dev/history/notes/PYTHON_IMPL_V0_11_NOTES.md)
for the full list. Highlights:

* Operator precedence is not in the normative spec (§11.1 defers to
  `docs/internals/parser.md` which is not normative). This is the
  single biggest v1.0-polish gap.
* `package <name>`, `export [c|js]? fn ...`, `fn ... = <expr>` body
  form, struct-field newline separator, `requires` post-signature
  clauses, `arena LABEL: <expr>` inline form — all appear in the
  examples but are not in the spec prose.
* Several smaller ambiguities around literal-suffix collisions
  (`5K` reserved), block-comment unterminated-error code, and the
  `///` vs `////` boundary.

## Known divergences from the Rust reference

This impl has not been bidirectionally diffed against `mty lex` /
`mty dump --cst` yet. The integrator's reconciliation pass is the
v1.0 follow-up.

Pre-known representational deviations:

* **Tree shape**: this impl produces JSON-friendly `dict` nodes
  with `_kind` discriminators. The Rust reference produces a
  rowan `GreenNode` plus a typed-AST view (`mty-ast`). Shapes
  will not byte-equal; structural-equivalence diffing is the
  correct comparison.
* **Trivia preservation**: both implementations preserve trivia,
  but storage models differ.
* **Diagnostic codes**: this impl mints codes within the §33 bands
  (`MT0xxx` lexer, `MT1xxx` parser) but the exact assignments
  inside each band may differ from the Rust reference.

All other observable behaviours are documented in
[`PYTHON_IMPL_V0_11_NOTES.md`](../../dev/history/notes/PYTHON_IMPL_V0_11_NOTES.md)
as findings or deviations.

## How to add a third independent implementation

Picking up a Go, OCaml, Zig, or other-language re-impl is a
two-to-three-week front-end-only project. The recommended approach:

1. **Read the spec first.** Open `docs/spec/v1.0-rc.md`. Do NOT
   open `crates/mty-syntax/` or `selfhost/`. If you must consult
   the Rust source to disambiguate, document each consultation as
   a finding.
2. **Use the Python impl's findings as a starting checklist.** The
   16 documented ambiguities in
   [`PYTHON_IMPL_V0_11_NOTES.md`](../../dev/history/notes/PYTHON_IMPL_V0_11_NOTES.md)
   tell you what spec text to scrutinise.
3. **Test against `examples/*.mty`.** Every example must lex and
   parse with zero diagnostics. This is the load-bearing
   validation — if your impl can't handle the official examples,
   the spec has a gap.
4. **Publish findings alongside the impl.** Each new impl's
   ambiguity list strengthens v1.x spec polish.
5. **Add a CI workflow** (model: `.github/workflows/python-impl.yml`)
   so the impl stays current when the spec or examples evolve.

## Roadmap

* **v0.11 (this slice)** — Python lexer + parser shipped.
* **v0.12 (next)** — Python agent/protocol/supervisor structural
  parse (close the deferred items in
  [`PYTHON_IMPL_V0_11_NOTES.md`](../../dev/history/notes/PYTHON_IMPL_V0_11_NOTES.md)).
* **v0.13+** — A real `mty dump --cst` diff harness. Once the
  Rust front-end can emit a stable JSON CST representation, the
  Python impl runs cross-impl conformance.
* **v1.1+** — Either extend the Python impl through HIR/typeck/
  borrow or pick a third-language impl (Go is the natural choice
  given the codebase's existing benchmark-comparator coverage).
