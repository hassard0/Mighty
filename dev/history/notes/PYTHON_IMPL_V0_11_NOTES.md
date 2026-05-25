# Python 2nd-impl notes — v0.11 swarm slice

This note accompanies the addition of `impl-py/`, a pure-Python
reference implementation of the Mighty front-end (lexer + parser).
The goal is to validate that the v1.0-RC2 spec is implementable from
**prose alone**, with no source-peeking into the Rust reference.

Per the v0.9 spec-freeze plan, having an independent 2nd
implementation is the single largest v1.0 freeze blocker. This is
that block landing.

## Setup and how to run

* **Python version**: 3.10 or newer (we use `match` statements and
  the walrus operator; pure stdlib otherwise).
* **Dev dep**: `pytest` (only required for the test harness).
* **Run**: from the workspace root:
  ```
  python -m pytest impl-py/tests/
  ```
* **Tests pass count at landing**: 135/135 passing on CPython 3.11
  (135 across `test_lexer.py`, `test_parser.py`, and
  `test_examples.py`).
* **Examples covered**: every example in `examples/` (01-20) lexes
  and parses with **zero diagnostics**. The swarm scope required
  01-05 only; the parser scope grew enough to handle the rest as a
  bonus (see the parser-scope expansion section below).

## Discipline followed

The package was built consulting ONLY these spec sources:

* `docs/spec/v1.0-rc.md` (the normative spec, especially §3 / §4 /
  §6.3 / §11 / §17.2 / §20.2 for parser shape; §3 for lexer)
* `docs/spec/v0.1-amendments.md` (cited but not deeply consulted)
* `docs/spec/CHANGELOG.md`
* The `examples/*.mty` corpus as test fixtures

The following were **not** opened during implementation, per the
swarm mandate:

* `crates/mty-syntax/`
* `crates/mty-ast/`
* anything under `selfhost/lexer/` or `selfhost/parser/`
* `docs/internals/parser.md` (the spec defers exact precedence here
  and notes A-grade compatibility with the parser internals doc; we
  reconstructed precedence from the conventional ladder, see Finding
  #6 below)

## Spec ambiguities discovered

The following are findings — interpretation calls we made where the
spec is silent or ambiguous. These are the load-bearing v1.0
spec-polish artefacts for the integrator to reconcile against the
Rust source.

### Finding #1 — Numeric underscore-separator placement

**Section**: §3.4.1.

**Spec text**:
```
INT_LITERAL = (DEC | HEX | OCT | BIN) ("_" (DEC | HEX | OCT | BIN))* SUFFIX?
```

**Ambiguity**: the grammar reads "(digits)(_digits)*", which forbids
a leading underscore and forbids two underscores in a row, but it
does NOT forbid a trailing underscore at end-of-literal (e.g. `1_`).
It also doesn't say what happens for `1__2` (two consecutive
underscores).

**Our decision**: forbid trailing underscores (emit `MT0006`), forbid
two underscores in a row (also `MT0006`). This matches the
conventional Rust-style rule.

### Finding #2 — Decimal-kilo `k` vs duration-minute `m`

**Section**: §3.4.4 / §3.4.5 / Amendment A1.

**Spec text**: "Lowercase `k` and uppercase `M` avoid collision with
the duration suffix `m` (= minutes). Uppercase `K` is reserved."

**Ambiguity**: what should the lexer do if it sees `5K`? "Reserved"
in this context could mean "rejected at lex time" or "accepted as an
identifier-continuation, so `5K` is `5` followed by `K`". We picked
the latter (treat `5K` as INT_LITERAL `5` + IDENT `K`) because the
size-suffix lookahead is *opt-in*: we only match the suffix if the
literal text appears in our allowlist.

**Sub-finding**: `5K` therefore lexes as two tokens; a Rust impl
that issues a diagnostic at lex time would diverge from us here.

### Finding #3 — HTML literal interpolation

**Section**: §3.4.3 / §22.2.

**Spec text**: "`html\"...\"` is a tagged template form that lowers
to a structured DOM fragment value (see §22)."

**Ambiguity**: at what layer does the `{name}` interpolation
placeholder split off — lexer, parser, or HIR lowering? The spec is
silent on the syntactic structure of the body.

**Our decision**: lex the whole `html"..."` blob as a single
`HTML_LITERAL` token; defer interpolation splitting to a later layer
(currently unimplemented). The lexer balances inner `{`/`}` so
nested braces inside the template don't terminate it early.

### Finding #4 — Block-comment depth on unterminated `/*`

**Section**: §3.2 ("nesting allowed").

**Ambiguity**: the spec doesn't enumerate a diagnostic code for an
unterminated nested block comment.

**Our decision**: emit `MT0004` (a code we minted under the §33
`MT0xxx` lexer band). A Rust-side reconciliation might use a
different code; this is a finding.

### Finding #5 — `///` vs `////`

**Section**: §3.2.

**Ambiguity**: is `////` a doc comment or a regular line comment?
The spec only lists `// `, `/* */`, and `///`.

**Our decision**: `///` *exactly* is a doc comment;
`////` and longer slash runs lex as regular line comments. This is
the conservative interpretation (a doc comment marker should be
exactly three slashes).

### Finding #6 — Operator precedence ladder

**Section**: §11.1 lists the expression forms but defers exact
operator precedence to `docs/internals/parser.md`, which is
**outside the consultable spec set** per the no-source-peek mandate.

**Ambiguity**: the precedence ordering of `&`, `|`, `^`, `<<`/`>>`,
the comparison family, and `&&`/`||` is not in the normative spec.

**Our decision**: adopt the conventional ladder (C/Rust order):
```
1: || or
2: && and
3: == !=
4: < <= > >=
5: |
6: ^
7: &
8: << >>
9: + -
10: * / %
```
The spec MAY codify a different ordering. This is the biggest
finding for v1.0 spec polish — operator precedence MUST be in the
normative document for cross-implementation determinism.

### Finding #7 — Postfix `?Msg`/`!Msg` "same source line" rule

**Section**: §3.5 (A12).

**Spec text**: "the propagate operator (`expr?`) and the ask/send
sugar (`expr?Msg(args)` / `expr!Msg(args)`) require their `Msg`
identifier on the same source line."

**Implementation choice**: we walk the original source slice between
the `?` (or `!`) token's end and the next non-trivia token's start;
if it contains a `\n` we treat the `?` as bare propagate. This is
the natural interpretation, BUT the spec doesn't say whether
comments-within count. A `//comment\n`  between `?` and `Msg` would
*also* contain a newline and would force the bare-propagate
interpretation — we adopt this conservative behaviour.

### Finding #8 — `package <name>` at top of file

**Section**: §4 has no `package` keyword.

**Observation**: `examples/19_backend_service.mty` begins with
`package search_api` and `examples/20_frontend_component.mty` begins
with `package counter_web`. `package` is not in the §3.3 reserved
keyword set.

**Our decision**: accept `package <ident>` as a permissive
top-of-file declaration (parsed as a special `package_decl` node).
This is a spec gap — the examples ship a construct the prose
doesn't define.

### Finding #9 — `export` and `export c fn ...` items

**Section**: §4 has no `export` keyword.

**Observation**: `examples/14_extern_c.mty` ships
`export c fn _add(a: I32, b: I32) -> I32 = a + b` and
`examples/20_frontend_component.mty` ships `export fn mount(...)`.
`export` is also not in §3.3.

**Our decision**: accept `export [c|js]? <fn-item>` as a permissive
wrapper that marks the inner fn item with `export = true` and an
optional `export_abi` field. This is a spec gap.

### Finding #10 — Bare `fn ... = <expr>` body form

**Section**: §4 / §5 (no spec mention).

**Observation**: example 14 ships `export c fn _add(a, b) = a + b`
with `=` instead of `{ ... }` as the body.

**Our decision**: accept `=` followed by a single expression as an
alternative fn body form. Spec gap.

### Finding #11 — Tuple-struct vs braced struct separator

**Section**: §4 (no explicit separator rules for struct fields).

**Observation**: `examples/02_struct_enum.mty` uses NEWLINES (not
commas) between struct fields:
```
struct User {
  id: UserId
  name: String
}
```
Spec doesn't say whether newlines separate fields or whether commas
are required.

**Our decision**: accept either — comma OR newline. This works for
our token stream because we strip trivia before structural parsing
and rely on the next token's class to determine end-of-field. Spec
gap: pick one and codify.

### Finding #12 — `requires` clauses on `unsafe fn`

**Section**: §21 doesn't enumerate function-precondition clauses.

**Observation**: example 17 ships
```
pub unsafe fn _from_raw(ptr: *U8, len: USize) -> Bytes
  requires ptr != null
  requires valid(ptr, len)
```
with `requires` clauses dangling after the signature, no body.

**Our decision**: parse `requires <expr>` clauses as auxiliary
post-signature `clauses` on the fn node. Spec gap: `requires` is
nowhere in the spec.

### Finding #13 — Sandbox / budget entry separator

**Section**: §16.1 / §16.2 example differences.

**Observation**: example 11 (`budget`) uses bare `wall 2s` (no `=`)
between key and value; example 18 (`sandbox`) uses `wall = 2s` with
`=`. Both are sibling constructs and the spec treats them similarly.

**Our decision**: accept both shapes (with or without `=`) inside a
unified `_parse_kv_brace_block` helper. Spec gap: pick one canonical
form.

### Finding #14 — `arena <label>: <expr>` inline form

**Section**: §10.1 only shows the braced `arena { ... }` form.

**Observation**: example 12 ships
```
fn turn_short(input: Str) -> Lowered!ParseErr {
  arena turn: lower(parse(tokenize(input))?)
}
```
with `: <expr>` instead of `{ ... }`.

**Our decision**: accept `arena LABEL : <expr>` as an
"arena_inline" node. Spec gap: arena's inline form is undocumented.

### Finding #15 — Anonymous protocol-message declaration syntax

**Section**: §13.1.

**Observation**: §13.1 shows `msg Inc(by: I32)`; examples 07 and 19
ship `Ping(msg: Str) -> Str` (no `msg` keyword) and `Query(q: Str)
-> Json!SearchErr`. Both shapes appear.

**Our decision**: agent/protocol bodies are in the deferred pile
(token-balanced but not parsed structurally), so we don't decide. A
v0.12 follow-up to parse them must pick one or accept both.

### Finding #16 — `macro` and `proc` not in reserved keywords

**Section**: §3.3 ("`proc` itself is **not** a reserved keyword;
the parser recognises the bigram") + macro-decl examples use
`macro <name>`.

**Observation**: §3.3 explicitly excludes `proc` and implicitly
excludes `macro` (it's not in the keyword list).

**Our decision**: handle `macro` and `proc macro` as
identifier-position keyword-likes in the parser, not as lexer-level
keywords. Matches the spec.

## Deviations from the Rust reference (Mighty `mty`)

This list is intentionally short: because we didn't read Rust source,
most deviations are pre-recorded as findings above. To compute a
real "diff" against `mty lex`/`mty dump --cst`, run them on the
example corpus and compare token kinds / CST shapes — left as a
follow-up exercise for the integrator.

Known representational deviations:

* **Tree shape**: we emit JSON-friendly `dict` nodes with `_kind`
  discriminators. The Rust parser produces a `GreenNode` (rowan) +
  typed-AST view (`mty-ast`). The shapes will not byte-equal.
* **Trivia preservation**: our lexer emits whitespace and comments
  as first-class tokens (with `is_trivia=True`); the rowan-based
  Rust lexer also keeps trivia, but the storage model differs.
* **Diagnostic codes**: we mint `MT0001-MT0007` for lexer errors
  and `MT1001-MT1004` for parser errors. These overlap with the
  spec's `MTxxxx` bands (§33) but the exact code assignments inside
  each band may not match the Rust impl (which has more granular
  codes for each fire condition).

## What's deferred (v0.12+ backlog)

The swarm scope listed "agents, protocols, supervisors, budgets,
arenas, async/spawn, macros" as deferred. Of those:

* **arenas**, **budgets**, **sandboxes**, **unsafe** — actually
  shipped at expression-position with full structural parsing (each
  example using them parses with zero diagnostics).
* **macros (decl)**, **proc macros** — shipped at item-level with
  brace-balanced body slurp. The macro body itself is NOT parsed as
  Mighty (it's a token-substitution template per §20.2; a fully-typed
  parse would be a structural-vs-template-DSL design decision).
* **agents**, **protocols**, **supervisors** — still deferred. They
  parse through as `deferred_agent` / `deferred_protocol` /
  `deferred_supervisor` items with their body tokens preserved as a
  string list. Full structural parsing is straightforward but the
  agent/protocol/supervisor surface is large (§12-§14) — estimated
  2-3 KLOC additional Python for a complete v0.12 pass.

Other deferred items:

* **HTML interpolation splitting** (Finding #3) — lexer treats the
  whole `html"..."` as one token; a future pass should split out
  the `{name}` placeholders into a sequence of LITERAL_FRAGMENT +
  IDENT + LITERAL_FRAGMENT tokens (or do this at the parser, where
  the §22.2 lowering rules apply).
* **Full Unicode `XID_Start`/`XID_Continue` tables** — we use
  `unicodedata.category()` which is close to but not byte-identical
  with the UCD `XID_Start` derived property. For ASCII source (which
  is every example) the two coincide; the gap matters only for
  exotic Unicode identifiers.
* **Borrow checking**, **type inference**, **HIR lowering**, **code
  generation** — none in scope here. These are still the v1.1+
  self-host roadmap per §39.

## Estimated effort to close

| Slice                                              | Est. LOC | Est. days |
|----------------------------------------------------|----------|-----------|
| Agent / protocol / supervisor structural parse     |   ~1500  |    1.5    |
| HTML interpolation split                           |    ~200  |    0.5    |
| Real `mty lex` / `mty dump --cst` diff harness     |    ~300  |    0.5    |
| HIR lowering                                       |   ~2500  |    3      |
| Sketch type checker                                |   ~3000  |    5      |
| Sketch borrow checker                              |   ~2000  |    4      |

Total to a "full front-end through borrow check" Python impl: ~9.5
KLOC, ~14 days. Out of scope for this swarm slice; tracked for v0.12+
prioritisation.
