# Spec RC3 — v0.12 polish notes

This note records the interpretation calls made when promoting the
v1.0-RC2 normative spec to **v1.0-RC3**, closing the three spec gaps
surfaced during v0.11 (KNOWN_ISSUES #10/#11/#12) and the 16 ambiguities
found by the Python 2nd-implementation work (see
`PYTHON_IMPL_V0_11_NOTES.md`).

The goal is **prose polish only**. No normative behaviour change. The
Rust reference compiler already implements every decision recorded
here; v0.12 just promotes the implementation choices into normative
prose so a third / fourth implementer can build to spec alone.

---

## Gap #10 — Operator precedence is now normative

**Problem:** v1.0-RC2 §11.1 referred readers to
`docs/internals/parser.md` for exact operator precedence. The internals
doc is **non-normative**: it documents *the Rust implementation*, not
the language. An independent implementer building from the spec alone
(as the Python 2nd impl did) cannot rely on it.

**Resolution:** the Pratt-style precedence table from
`crates/mty-syntax/src/parser/exprs.rs::infix_bp` is promoted verbatim
to a new normative subsection §11.1.1 "Operator precedence". The
table mirrors the C/Rust convention and was already what the Rust
parser shipped — this is documentation-only.

Cross-references:

- `docs/internals/parser.md` now points at the normative spec for the
  authoritative table (the internals doc keeps a one-line summary).
- `PYTHON_IMPL_V0_11_NOTES.md` Finding #6 ("operator precedence
  ladder") is closed by this addition.
- KNOWN_ISSUES.md item #10 is marked **resolved in v1.0-RC3**.

The right-associativity of assignment + compound-assignment
(`= += -= *= /= %= &= |= ^= <<= >>=`) is also normative now (table
footnote). All other binary operators are left-associative.

---

## Gap #11 — Six FROZEN typeck codes are constructor-only

**Problem:** MT2003, MT2009, MT2022, MT2023, MT2024, MT2025 are defined
with full explain text in `crates/mty-diagnostics/src/codes.rs` and
listed as FROZEN in `docs/spec/conformance-coverage.md`, but no emit
site exists in any crate. The spec promises diagnostics that the
compiler does not produce.

Each code was reviewed for whether it describes a real condition that
USERS will hit. Decision per code:

| Code   | Title                          | Decision | Rationale |
|--------|--------------------------------|----------|-----------|
| MT2003 | CANNOT_INFER_TYPE              | RETAIN (FROZEN — emit-site landing in v1.x) | Real future need. Inference can stall on `let x = vec.iter().collect()` style; v1.x trait-based iterator + collect chain will hit this. |
| MT2009 | UNKNOWN_VARIANT                | RETAIN (FROZEN — emit-site landing in v1.x) | Real user-facing typo (`Color.Reed` for `Color.Red`). Currently MT2007 / MT2021 funnels these. v1.x enum-aware resolver should split MT2009 out. |
| MT2022 | NOT_A_STRUCT                   | RETAIN (FROZEN — emit-site landing in v1.x) | Real user error (`SomeFn{a:1}` where SomeFn is not a struct). Currently swallowed by MT2002 path. v1.x will split. |
| MT2023 | GENERIC_ARG_MISMATCH           | RETAIN (FROZEN — emit-site landing in v1.x) | Lifetime-where-type kind mismatch is a real condition once lifetimes ship (RFC-006-ish). MVP only has types, so the code does not fire today. |
| MT2024 | LAMBDA_ARITY_MISMATCH          | RETAIN (FROZEN — emit-site landing in v1.x) | Lambdas exist; arity check currently funnels into the generic MT2005. Splitting into MT2024 is purely a UX improvement, not a behaviour change. |
| MT2025 | CANNOT_TAKE_REF                | RETAIN (FROZEN — emit-site landing in v1.x) | Real user error (`&literal`, `&fn_call()`). Currently the borrow checker accepts the temporary via implicit promotion. v1.x stricter pass will emit MT2025. |

All six describe valid future conditions; none are vestigial. The
decision is **#2 from the task brief** (RETAIN as "FROZEN — emit landing"),
applied to every code.

The parallel **conformance-closure** swarm agent may add emit sites
for any of these codes; if they do, the per-code row in
`docs/spec/conformance-coverage.md` should flip from `gap` to
`covered`, and the corresponding "v1.x emit-site landing" entry in
KNOWN_ISSUES.md can be struck through. This RC3 spec polish does NOT
emit; we only document the FROZEN-with-future-emit decision.

KNOWN_ISSUES item #11 gets a per-code action table in lieu of being
fully closed; closure happens slice-by-slice as each code lands its
emit site.

---

## Gap #12 — `package`, `export`, `requires` and other contextual keywords

**Problem:** the Python 2nd impl flagged that `package`, `export`, and
`requires` appear in the example corpus but are not in the §3.3
reserved keyword set. The Rust impl recognises them as
**lexer-level keywords** (they have `#[token("...")]` entries in
`syntax_kind.rs`); the §3.3 reserved list was simply incomplete.

**Resolution:** §3.3 is rewritten to enumerate the keyword set
accurately, and to split it into:

1. **Reserved keywords** — always lex as keywords, never available as
   identifiers (this is the existing list, expanded with the missing
   names from the Rust lexer).
2. **Contextual keywords** — words that the parser recognises only in
   specific positions (typically an item-start position or a clause
   keyword). Outside those positions they remain ordinary identifiers.

Contextual keywords enumerated from the Rust parser (text-comparison
sites, not `#[token]` sites):

| Word         | Context                                               | Source                                    |
|--------------|-------------------------------------------------------|-------------------------------------------|
| `proc`       | `proc macro` bigram at item start                     | `crates/mty-syntax/src/parser/items.rs` (existing in §3.3) |
| `supervisor` | item start (interchangeable with the `sup` keyword)   | `crates/mty-syntax/src/parser/items.rs:76` |
| `component`  | inside `extern <component>` declarations              | `crates/mty-syntax/src/parser/extern_.rs:82` |
| `v0`/`v1`/`v2`/… | optional protocol-version tag after `protocol Name` | `crates/mty-syntax/src/parser/agents.rs:127` |

The lexer keywords `package`, `export`, `requires`, `agent`,
`protocol`, `sup`, `sandbox`, `child`, `on_fail`, `restart`, `backoff`,
`up_to`, `detach`, `macro`, `run`, `join`, `scope`, `dyn`, `derive`
were missing from the §3.3 list. They are added in this RC3.

Removed from §3.3 (incorrect entries):

- `and` / `or` — not in the Rust lexer; the language uses `&&` / `||`.
  Listed in v1.0-RC2 §3.3 in error.
- `init` / `deinit` / `panic` / `static` / `union` — listed as
  reserved but not present in the Rust lexer's keyword set. v1.0
  reserves the **names** (we want them available for a future
  extension) but they are not keywords today. Moved to a "reserved
  for future use" subsection so an implementer doesn't reject them as
  identifiers.
- `Self` — capitalized; still reserved but called out as a special
  type-position keyword (it is not in the lexer keyword table either;
  the parser routes the bare ident in type position).

(See PYTHON_IMPL_V0_11_NOTES.md Findings #8, #9, #12, #16 for the
original surfacing.)

KNOWN_ISSUES item #12 is marked **resolved in v1.0-RC3**.

---

## 16 Python-impl ambiguities — disposition

Each finding from `PYTHON_IMPL_V0_11_NOTES.md` gets a one-line
disposition. Findings #6, #8, #9, #12 (#15 partly), #16 are covered by
the Gap #10 / Gap #12 work above. The rest:

| # | Topic                                          | Disposition                                                                                 |
|---|------------------------------------------------|---------------------------------------------------------------------------------------------|
| 1 | Numeric underscore-separator placement          | **Adopted Python interpretation.** RC3 §3.4.1 forbids leading underscores, trailing underscores, and consecutive underscores; emit `MT0006`. |
| 2 | `5K` (`K` reserved) lex behaviour              | **Adopted Python interpretation.** RC3 §3.4.4 clarifies: unrecognized size-suffix letters lex as IDENT continuation, not as lex errors. `5K` is `INT_LITERAL 5` + `IDENT K`. |
| 3 | HTML literal interpolation layer                | **DEFERRED to RC4 / v1.x.** Brief note added in §3.4.3 + §22.2 that the body lexes as a single `HTML_LITERAL` blob; interpolation splitting layer is **v1.0 OPEN — deferred to v1.1+** and tracked as new amendment A110 candidate. |
| 4 | Unterminated nested block-comment diagnostic    | **Codified.** RC3 §3.2 explicitly assigns `MT0004 unterminated_block_comment` to this case. |
| 5 | `///` vs `////`                                | **Adopted Python interpretation.** RC3 §3.2 specifies: exactly 3 slashes = doc comment; 4+ slashes = ordinary line comment. |
| 6 | Operator precedence ladder                     | **Closed by Gap #10.** Precedence table is normative in §11.1.1. |
| 7 | Postfix `?Msg`/`!Msg` same-line + comments     | **Codified.** RC3 §3.5 footnote clarifies: a newline in *any* trivia (whitespace OR comments) between the `?`/`!` and the next non-trivia token forces the bare-propagate/bare-not interpretation. |
| 8 | `package <name>` keyword                       | **Closed by Gap #12.** `package` is added to §3.3 reserved keyword set; §4 documents the syntax. |
| 9 | `export` keyword                                | **Closed by Gap #12.** `export` is added to §3.3 reserved keyword set; §17.2 documents `export fn` and `export c fn`. |
| 10| Bare `fn ... = <expr>` body form               | **Codified.** RC3 §4 adds the `=` body alternative as normative shorthand: `fn f(...) -> T = <expr>` is equivalent to `fn f(...) -> T { <expr> }`. |
| 11| Struct field separator (newline vs comma)      | **Codified.** RC3 §4 / §6.2 says either separator is accepted; a single struct may mix them. Matches Rust impl. |
| 12| `requires` clauses on `unsafe fn`              | **Closed by Gap #12 (keyword) + new §21.x.** `requires <expr>` is added to §21 as the function-precondition clause syntax. Multiple clauses allowed; each is a boolean expression. Currently parse-only (no runtime enforcement); upgrade tracked as v1.x. |
| 13| Sandbox / budget entry separator (`=` vs none) | **Codified — both accepted.** RC3 §16 footnote: `key value` and `key = value` are equivalent inside `sandbox` / `budget` blocks. |
| 14| `arena <label>: <expr>` inline form            | **Codified.** RC3 §10.1 adds the inline form `arena LABEL : <expr>` as an alternative to the braced `arena LABEL { <stmts> }`. Lowers to a single-expr arena scope. |
| 15| Anonymous protocol-message decl (`Ping(...)`)  | **Codified — both accepted.** RC3 §13.1 says: inside a protocol body, a bare `Name(params) -> Ret` line declares a message; the `msg` keyword prefix is optional sugar for readability. |
| 16| `macro` / `proc` keyword treatment              | **Closed by Gap #12.** `macro` is added to §3.3 reserved keyword set; `proc` stays as the only contextual-bigram (`proc macro`). |

No finding requires more design work than a paragraph-level spec edit.
Nothing is deferred to RC4 except Finding #3 (HTML interpolation
layer), which is a real design question and gets a new amendment
A110 reference.

---

## v1.0-RC3 cross-impl test plan

After RC3 ships, a third implementer (e.g. the OCaml or Go front-end
swarm) should be able to:

1. Build a lexer using §3 alone, with no source-peek into the Rust
   crates. The §3.3 keyword set must be complete; the §3.4 literal
   grammars must be unambiguous (Findings #1, #2, #4, #5 close the
   v0.11 gaps).
2. Build a parser using §11 + §4 + §6 alone. The §11.1.1 precedence
   table replaces the dependency on `docs/internals/parser.md`.
3. Re-derive the exact diagnostic codes from §33 (which carries the
   MT-band registry). The six "FROZEN — emit-site landing in v1.x"
   codes are flagged in their explain text so an implementer knows
   they describe future conditions.

---

## Files modified by this slice

- `docs/spec/v1.0-rc.md` — title bump to RC3; §3.2, §3.3, §3.4, §3.5,
  §4, §10.1, §11.1.1 (new), §13.1, §16, §21.x edits.
- `docs/spec/v0.1-amendments.md` — cross-reference to RC3 in the
  status legend; A110 candidate placeholder.
- `docs/spec/CHANGELOG.md` — v1.0-RC3 entry appended.
- `KNOWN_ISSUES.md` — items #10, #11, #12 added to the catalog and
  marked resolved-in-RC3 (item #11 keeps a per-code action table).
- `dev/history/notes/SPEC_RC3_V0_12_NOTES.md` — this file.

No crate source modified. No changes to runtime or compiler behaviour.
