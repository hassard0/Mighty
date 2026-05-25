# Mighty Spec Changelog

Chronological log of changes to the Mighty language specification.
Each entry references the slice / release in which the change shipped
and the originating amendment number (`Axx`) where applicable.

Status codes match the
[`docs/spec/v0.1-amendments.md`](v0.1-amendments.md) legend:

- **F** = FROZEN (folded into v1.0-RC verbatim, stable v1.x)
- **S** = SUPERSEDED (later amendment took over; see cross-reference)
- **O** = OPEN (v1.0 ships as-is; v1.1+ may evolve)
- **R** = REVERTED (none yet)

The consolidated normative spec is
[`docs/spec/v1.0-rc.md`](v1.0-rc.md). Reconciliation notes for
contradictions resolved during consolidation are at
[`SPEC_CONSOLIDATION_V0_8_NOTES.md`](https://github.com/hassard0/Mighty/blob/main/SPEC_CONSOLIDATION_V0_8_NOTES.md).

---

## v0.1 (slices 1..8) — initial spec ladder walk

The 39-section `stardust_language_spec_v0_1.md` was the seed document.
Slices 1..8 implemented the ladder; the amendments below were adopted
en route. No amendment is **R** — every decision either landed or got
superseded by a later, better-shaped one.

### Slice 2 (v0.1 surface — lexer/parser/CST)

| Amendment | Status | Title                                                  |
|-----------|--------|--------------------------------------------------------|
| A1        | F      | Decimal size-literal suffixes `k` and `M`              |
| A2        | F      | Expression-position turbofish `Path::[T1, T2]`         |
| A3        | F      | Keyword-tolerant `.method` and `.field`                |
| A4        | F      | Keyword-tolerant `effect` names                        |
| A5        | F      | `run <expr>` as an expression form                     |
| A6        | F      | `if let Pattern = expr { ... }`                        |

### Slice 3 (v0.1 surface — type checker)

| Amendment | Status | Title                                                  |
|-----------|--------|--------------------------------------------------------|
| A7        | F      | `?` strictly requires Result-returning enclosing fn    |
| A8        | S → A19 | Integer-literal defaulting to `I32` (deferred)        |
| A9        | F      | Primitive type names in both namespaces                |
| A10       | S → A17 + A65 | Built-in method table (residual permissive only) |
| A11       | O      | Anonymous error unions `T!{A,B}` → `Result[T, Error]`  |
| A12       | F      | Postfix `?`/`!` require `Msg` ident on same line       |

### Slice 4 (v0.1 surface — borrow checker)

| Amendment | Status | Title                                                  |
|-----------|--------|--------------------------------------------------------|
| A13       | S → A26 | Hardcoded `Copy` set (replaced by `#[derive(Copy)]`)  |
| A14       | S → A65.b | Conservative `Sendable` set                         |
| A15       | O      | Arena escape: direct-naming MVP                        |
| A16       | F      | Match exhaustiveness promoted to Error                 |
| A17       | F      | Method dispatch policy (inherent first, then traits)   |
| A18       | S → A28 + A65.c | Protocol-aware agent handler params (Warning) |
| A19       | F      | Integer/float defaulting pass (closes A8)              |
| A20       | S → A55 | Lexical borrow regions (Rust 2015 style)              |
| A21       | S → A65 | Scope-aware tolerance for unresolved values           |

### Slice 5 (v0.1 surface — effects + traits + sandboxes)

| Amendment | Status | Title                                                  |
|-----------|--------|--------------------------------------------------------|
| A22       | F      | Effect inference algorithm (bottom-up + fixpoint)      |
| A23       | F      | Capability narrowing constraints                       |
| A24       | F      | Trait coherence (name-only) + dispatch                 |
| A25       | F      | `dyn Trait` object safety                              |
| A26       | F      | Derive set (`Copy`, `Hash`, `Eq`)                      |
| A27       | S → A43 | Top-level `sandbox` items (metadata-only at parse)    |
| A28       | F      | Strict protocol-handler checks (supersedes A18)        |
| A29       | S → A56 | `move *ref` reserved as MT3009                        |
| A30       | F      | Strict-profile `alloc` ban                             |

### Slice 6 (v0.1 surface — interpreter)

| Amendment | Status | Title                                                  |
|-----------|--------|--------------------------------------------------------|
| A31       | O      | Arena runtime enforcement deferred                     |
| A32       | S → async dispatch | Slice-6 agent dispatch is synchronous     |
| A33       | F      | Effect calls dispatched via Host trait                 |
| A34       | S → A43 + A70 + A99 | Budgets + sandboxes are metadata only    |
| A35       | F      | Slice-6 interpreter is single-thread deterministic     |

### Slice 7 (v0.1 surface — runtime)

| Amendment | Status | Title                                                  |
|-----------|--------|--------------------------------------------------------|
| A36       | S → A96 | `std.http.serve` MVP shape (in-memory)                |
| A37       | S → A50 + A99 | Slice-7 memory budget approximation             |
| A38       | S → A71 | Telemetry JSON schema (OTLP-flavoured)                |
| A39       | F      | Deterministic mode (`RuntimeBuilder::deterministic`)   |
| A40       | F      | Mailbox defaults (depth 1024, policy Block)            |
| A41       | S → A70 | Slice-7 cancellation semantics (between-turn only)    |
| A42       | F      | `restart up_to N in DUR` semantics                     |
| A43       | F      | Top-level sandbox executes as a child runtime          |
| A44       | F      | Slice-7 deref-of-ref write path                        |
| A45       | O      | `mty run --legacy-interp` opt-out                      |

### Slice 8 (v0.1 surface — codegen)

| Amendment | Status | Title                                                  |
|-----------|--------|--------------------------------------------------------|
| A46       | F      | Cranelift-only native backend; LLVM scaffolded         |
| A47       | O      | Wasm Component Model deferred (core modules only)      |
| A48       | F      | `mty run` defaults to JIT (Cranelift)                  |
| A49       | O      | Per-(fn, type-args) monomorphisation (strip in MVP)    |
| A50       | F      | `bumpalo`-backed arenas with byte-charging             |
| A51       | F      | Codegen trap codes MT8001..MT8010                      |
| A52       | F      | Native linker discovery order                          |
| A53       | F      | `extern { fn ... }` resolved via `libloading`          |

---

## v0.3 — soundness pass

The first post-v0.1 release. Goal: harden the soundness of borrows,
cancellation, telemetry, mailbox shape, and scope-strictness.

| Amendment | Status | Title                                                  |
|-----------|--------|--------------------------------------------------------|
| A54       | F      | Field-level borrow tracking via Place algebra          |
| A55       | F      | NLL last-use deactivation (supersedes A20)             |
| A56       | F      | Precise MT3009 for `move *ref` (supersedes A29)        |
| A65       | F      | Scope-aware permissive/strict type-check policy        |
| A65.b     | F      | Sendable trait at cross-agent message sites            |
| A65.c     | F      | MT4031 strict handler param-type check                 |
| A65.d     | F      | `core` profile rejects `alloc` (ratified)              |
| A70       | F      | Cooperative mid-turn cancellation (supersedes A41)     |
| A71       | F      | OTLP wire-format telemetry (supersedes A38)            |
| A72       | F      | Slab-pool mailbox frames                               |
| A73       | F      | Batched per-turn deadline scheduler                    |

Note on numbering: A57..A64 and A66..A69 are skipped (reserved slots
used by intermediate drafts during v0.3 development that did not ship).

---

## v0.4 — dogfood + ecosystem

Goal: stand up a real package manager, declarative macros, doc gen,
and the registry surface.

No new spec amendments. Highlights:

- `mty-pkg` package manager + `mighty.lock` resolution.
- Declarative macros + hygienic mangling (precursor to A92).
- `mty doc` HTML generator.
- Registry index protocol shaped per [§36.3](v1.0-rc.md#363-registry-protocol).
- `mty new` package scaffolder.

See [`RELEASE-v0.4.md`](https://github.com/hassard0/Mighty/blob/main/RELEASE-v0.4.md) and
[`REGISTRY_V0_4_NOTES.md`](https://github.com/hassard0/Mighty/blob/main/REGISTRY_V0_4_NOTES.md).

---

## v0.5 — self-host lexer + dogfood completion + LSP advanced

Goal: prove self-hosting by porting the lexer, close v0.4 dogfood
gaps, and expand the LSP capability surface.

### LSP

| Amendment | Status | Title                                                  |
|-----------|--------|--------------------------------------------------------|
| A74       | F      | LSP v0.5 capability expansion (semantic tokens, rename, inlay hints, code actions, signature help, semantic completion) |

### Control flow

| Amendment | Status | Title                                                  |
|-----------|--------|--------------------------------------------------------|
| A80       | F      | `break` / `continue` as real HIR nodes                 |
| A81       | F      | Iterator protocol via `__sdust_iter_next`              |
| A82       | F      | Loop back-edge fixed-point in the borrow checker       |

### Macros

| Amendment | Status | Title                                                  |
|-----------|--------|--------------------------------------------------------|
| A90       | F      | `name!(args)` macro invocation marker                  |
| A91       | F      | MT6001 `unknown_macro` activated                       |
| A92       | F      | Extended hygiene mangling (tuple/struct/ref patterns)  |
| A93       | F      | Cross-file `pub macro`                                 |
| A94       | O      | Procedural macros (parse + store + purity check)       |
| A95       | F      | Standard macro library shipped with mty-macros         |

### Dogfood gap closures

| Amendment | Status | Title                                                  |
|-----------|--------|--------------------------------------------------------|
| A96       | F      | `std.http.serve` binds a real socket (supersedes A36)  |
| A97       | O      | `mighty:web/dom` interface added (canonical-ABI bridge OPEN) |
| A98       | F      | Str method table real impls                            |
| A99       | F      | `RunResult::MemBudgetExceeded` + memory auto-charging  |
| A100      | F      | FsCap allowlist enforcement (process-wide default cap) |

Note: A75..A79 and A83..A89 are skipped (reserved during v0.5
development; not used).

---

## v0.6 — multi-core scheduler + benchmarks + self-host parser

Goal: distribute work across N cores, ship first honest benchmarks,
and port the parser to Mighty.

### Scheduler

| Amendment | Status | Title                                                  |
|-----------|--------|--------------------------------------------------------|
| A101      | F      | v0.6 multi-worker scheduler (N tokio + crossbeam-deque) |
| A102      | O      | Agent affinity hints (runtime API only; syntax v1.1+)  |
| A103      | O      | Lightweight migration (routing-table; lossless v1.1+)  |
| A104      | F      | Per-worker scheduler telemetry                         |
| A105      | F      | Scheduler driver runtime separation                    |
| A106      | F      | Default worker count = `available_parallelism()`       |

### Integrator easy wins

| Amendment | Status | Title                                                  |
|-----------|--------|--------------------------------------------------------|
| A107      | F      | Central diagnostic catalog for MT6001-MT6006           |
| A108      | F      | `BuiltinId::DomOp(name)` MtyIR variant (closes v0.5 deferral #6) |
| A109      | F      | Per-call FsCap isolation contract                      |

---

## v0.7 — brand rename (Stardust → Mighty)

**Tag:** `v0.7.0-rebrand`. **Commits:** `b83673a` → `36b3140`.

No new spec amendments. The rebrand consolidates four identifier
sweeps:

| Old                                   | New                                  |
|---------------------------------------|--------------------------------------|
| Language name `Stardust`              | `Mighty`                             |
| CLI binary `sdust`                    | `mty`                                |
| Source extension `.sd`                | `.mty`                               |
| Interface extension `.sdi`            | `.mtyi` (no files yet existed)       |
| Manifest filename `star.toml`         | `mighty.toml`                        |
| Lockfile `star.lock`                  | `mighty.lock`                        |
| IR name `SIR`                         | `MtyIR`                              |
| Diagnostic prefix `SDxxxx`            | `MTxxxx`                             |
| Rowan language type `Stardust`        | `Mighty`                             |
| Profile dir `.stardust/`              | `.mighty/`                           |
| Crate prefix `sdust-*`                | `mty-*`                              |
| WIT namespaces `stardust:caps/*`, `stardust:web/*` | `mty:caps/*`, `mty:web/*` |

GitHub repo URL was later renamed `hassard0/stardust` →
`hassard0/Mighty` (the on-disk directory may still be `stardust` on
some clones; both resolve to the same repo).

Preserved:

- `edition = "2026"` (calendar year, not brand)
- `wasi:*` WIT namespace (upstream WASI types)
- `STARDUST_*` env var prefix (back-compat with v0.6 deployments)
- `pkg.stardust.dev` registry URL constant (until a Mighty-branded
  registry comes online)

See [`REBRAND_NOTES.md`](https://github.com/hassard0/Mighty/blob/main/REBRAND_NOTES.md) and
[`RENAME_LOG.md`](https://github.com/hassard0/Mighty/blob/main/RENAME_LOG.md) for the full interpretation
log.

Back-compat shims:

- `mty explain` accepts `MT0001` (canonical) AND `SD0001` (legacy).
- `mty dump --sir` is a clap alias of `mty dump --ir`.
- The `.sd` source extension still parses (deprecation review v1.2,
  removal v2.0 earliest).

---

## v0.8 — spec consolidation (v1.0-RC)

This release. Goal: fold the 88 amendments into a normative v1.0
release candidate spec.

Outputs:

- [`docs/spec/v1.0-rc.md`](v1.0-rc.md) — 2500+ line normative spec.
- [`docs/spec/v0.1-amendments.md`](v0.1-amendments.md) — every
  amendment carries a `**Status:**` line (FROZEN / SUPERSEDED / OPEN /
  REVERTED).
- [`docs/spec/CHANGELOG.md`](CHANGELOG.md) — this file.
- [`SPEC_CONSOLIDATION_V0_8_NOTES.md`](https://github.com/hassard0/Mighty/blob/main/SPEC_CONSOLIDATION_V0_8_NOTES.md) —
  reconciliation notes for contradictions resolved during
  consolidation.

Classification totals:

| Status     | Count |
|------------|-------|
| FROZEN     | 63    |
| SUPERSEDED | 15    |
| OPEN       | 10    |
| REVERTED   | 0     |
| **Total**  | **88**|

No normative behaviour changed in v0.8. The consolidation is
docs-only.

---

## v1.0-RC2 (v0.9 spec-freeze prep, 2026-05-24)

The v0.8 consolidation classified 10 amendments as **OPEN**. v1.0-RC2
resolves each one — three reclassified to **FREEZE-MVP** (the MVP
behaviour is the v1.0 normative contract; v1.1+ extensions land as
additive amendments), seven retained as **DEFER-V1.1** with explicit
RFC tracking — and ships six first-draft RFCs covering the
architecturally-significant v1.1+ promotion paths.

No normative behaviour changed in v0.9. The diff against RC1 is
restricted to:

- Per-amendment `**Status:**` updates in `docs/spec/v0.1-amendments.md`.
- Spec title bump (`v1.0-RC` → `v1.0-RC2`) and §A.1 / §A.2 prose
  promotion in `docs/spec/v1.0-rc.md`.
- New RFC directory `docs/spec/rfcs/` with six RFCs.
- New `SPEC_FREEZE_V0_9_NOTES.md` at repo root documenting the
  resolutions + v1.0 freeze plan + freeze blockers + post-v0.9 work.

### OPEN-amendment resolutions

| Amendment | RC1 status | RC2 resolution | RFC      |
|-----------|------------|----------------|----------|
| A11       | OPEN       | DEFER-V1.1     | RFC-001  |
| A15       | OPEN       | FREEZE-MVP     | —        |
| A31       | OPEN       | FREEZE-MVP     | —        |
| A45       | OPEN       | DEFER-V1.1     | —        |
| A47       | OPEN       | DEFER-V1.1     | RFC-002  |
| A49       | OPEN       | FREEZE-MVP     | —        |
| A94       | OPEN       | DEFER-V1.1     | RFC-003  |
| A97       | OPEN       | DEFER-V1.1     | RFC-002  |
| A102      | OPEN       | DEFER-V1.1     | RFC-005  |
| A103      | OPEN       | DEFER-V1.1     | RFC-006  |

A100 was FROZEN at RC1 (process-wide-default cap is the v1.0
normative contract) but carried a v1.1+ residual tag for MtyIR-lower
per-call cap threading. RC2 adds [RFC-004](v1.0-rc.md#c1-rfcs-by-amendment-rc2)
to track that residual without reclassifying A100 itself.

### RFCs drafted

| RFC      | Covers                  | Target | Comment window |
|----------|-------------------------|--------|----------------|
| RFC-001  | A11 first-class union ADTs                  | v1.1     | 30 days |
| RFC-002  | A47 + A97 Wasm Component Model wrapper      | v1.1     | 60 days |
| RFC-003  | A94 sandboxed proc-macro execution          | v1.1     | 30 days |
| RFC-004  | A100 carryover + A109 per-call FsCap        | v1.1     | 30 days |
| RFC-005  | A102 agent affinity front-end syntax        | v1.1     | 14 days |
| RFC-006  | A103 lossless live agent migration          | v1.1 or v1.2 | 60 days |

### Classification totals at RC2

| Status              | Count |
|---------------------|-------|
| FROZEN              | 63    |
| FREEZE-MVP          | 3 (A15, A31, A49) |
| SUPERSEDED          | 15    |
| DEFER-V1.1          | 7 (A11, A45, A47, A94, A97, A102, A103) |
| REVERTED            | 0     |
| **Total**           | **88**|

(FROZEN and FROZEN-MVP carry the same v1.x stability commitment. The
"MVP" qualifier signals that a v1.1+ additive extension is planned;
the MVP behaviour itself never changes.)

---

## v1.0-RC3 (v0.12 spec polish, 2026-05-25)

The v0.11 Python 2nd implementation surfaced three normative gaps in
the RC2 spec and sixteen interpretation ambiguities the impl had to
guess at. v1.0-RC3 closes all three gaps and codifies a decision for
every ambiguity. **No normative behaviour changed in v0.12** — the
Rust reference compiler already implements every decision here; RC3
just promotes those implementation choices into normative prose so a
third or fourth front-end implementation can build to spec alone.

### Gaps closed (originating: KNOWN_ISSUES #10 / #11 / #12)

| Issue | Topic                                          | RC3 resolution |
|-------|------------------------------------------------|----------------|
| #10   | Operator precedence not normative              | New §11.1.1 promotes the Pratt precedence table from `crates/mty-syntax/src/parser/exprs.rs` verbatim. `docs/internals/parser.md` now points at the spec for the authoritative table. |
| #11   | Six FROZEN typeck codes are constructor-only   | Per-code disposition in new §33.1: all six RETAINED as "FROZEN — emit-site landing in v1.x". Each describes a real future-emit condition; code-points + explain text are stable; the Rust compiler funnels them into more general codes today. |
| #12   | `package` / `export` / `requires` + other lexer keywords missing from §3.3 | §3.3 rewritten into §3.3.1 reserved keywords (full 63-word v1.0 list including the booleans), §3.3.2 contextual keywords (4 positions), §3.3.3 reserved for future use (7 names the spec reserves but the v1.0 lexer treats as identifiers). §4.1, §21.1, §26.2 add per-keyword syntax sections. |

### Python-impl ambiguities resolved (originating: PYTHON_IMPL_V0_11_NOTES.md)

All 16 findings have a normative disposition (full table in
`SPEC_RC3_V0_12_NOTES.md`). Findings #6, #8, #9, #12, #16 are
covered by the gap-closure work above. The remaining 11:

| # | Topic                                          | RC3 spec edit |
|---|------------------------------------------------|---------------|
| 1 | Underscore-separator placement in INT_LITERAL   | §3.4.1 — `MT0006` for leading / trailing / consecutive underscores. |
| 2 | `5K` lex behaviour (`K` reserved)              | §3.4.4 — unrecognised suffix letters lex as IDENT continuation; no lex-time diagnostic. |
| 3 | HTML literal interpolation layer                | §3.4.3 + §22.2 — body lexes as single HTML_LITERAL token; interpolation split is v1.0 OPEN, candidate A110. |
| 4 | Unterminated block-comment diagnostic           | §3.2 — `MT0004 unterminated_block_comment`. |
| 5 | `///` vs `////`                                | §3.2 — exactly 3 slashes = doc comment; 4+ = ordinary line comment. |
| 7 | Postfix `?Msg` same-line + trivia comments      | §3.5 / §11.1.1 footnote — newline in ANY trivia (whitespace or comments) forces bare-propagate interpretation. |
| 10| Bare `fn ... = <expr>` body form               | §4.4 — promoted to normative shorthand. |
| 11| Struct field separator (newline vs comma)      | §4.5 — both accepted; a single body may mix. |
| 13| Sandbox / budget entry separator (`=` vs none) | §16.2 — both shapes accepted; `=` recommended. |
| 14| `arena <label>: <expr>` inline form            | §10.1 — promoted to normative alternative. |
| 15| Anonymous protocol-message decl (`Ping(...)`)  | §13.1 — bare form and `msg`-prefixed form both accepted. |

### Diff summary (RC2 → RC3)

- `docs/spec/v1.0-rc.md`: title bump RC2 → RC3; new §11.1.1 (operator
  precedence table); rewritten §3.3 (keyword set, +20 keywords);
  expanded §3.2 / §3.4 / §3.5 (lexer ambiguities closed); new §4.1
  (`package` decl), §4.4 (`= <expr>` body form), §4.5 (field
  separators); expanded §10.1 (arena inline form); expanded §13.1
  (bare `msg` form); expanded §16.2 (entry-separator forms); new
  §21.1 (`requires` clauses); new §26.2 (`export <abi> fn`); new
  §33.1 (FROZEN typeck codes deferred emit-site).
- `docs/spec/v0.1-amendments.md`: status-legend section cross-refs
  RC3; candidate amendments A110 / A111 noted as deferred-to-v1.1+.
- `docs/internals/parser.md`: precedence section now points at
  normative §11.1.1.
- `KNOWN_ISSUES.md`: items #10, #11, #12 added to the catalog. #10
  and #12 marked **resolved in v1.0-RC3**; #11 carries a per-code
  action table and remains open until each emit site lands.
- `dev/history/notes/SPEC_RC3_V0_12_NOTES.md`: NEW — per-finding
  disposition rationale.

### Candidate amendments tracked (not yet allocated)

- **A110** — HTML-literal interpolation lexing layer. Splits
  `{name}` placeholders from the `HTML_LITERAL` blob. Target v1.1.
- **A111** — Runtime / static enforcement of `unsafe fn` `requires`
  clauses. Target v1.x (likely after the broader verifier work).

Neither is allocated a number in `docs/spec/v0.1-amendments.md` until
a v0.12+ slice ships the corresponding behaviour.

### Classification totals at RC3 (unchanged from RC2)

| Status              | Count |
|---------------------|-------|
| FROZEN              | 63    |
| FREEZE-MVP          | 3 (A15, A31, A49) |
| SUPERSEDED          | 15    |
| DEFER-V1.1          | 7 (A11, A45, A47, A94, A97, A102, A103) |
| REVERTED            | 0     |
| **Total**           | **88**|

---

## v1.0 (planned) — release stability

After the v1.0-RC3 bake-in period, v1.0 stable ships with:

- The FROZEN + FROZEN-MVP matrix locked.
- The DEFER-V1.1 matrix continues to evolve in v1.x minor releases
  per
  [Appendix B](v1.0-rc.md#appendix-b--backwards-compatibility-policy).
- The 6 RFCs enter their public comment windows; acceptance drives
  the v1.1 amendments.

Proposed v1.0 freeze date: 2026-09-01 (earliest plausible). Triggers
documented in `SPEC_FREEZE_V0_9_NOTES.md` (RFC comment windows
opened, external re-implementation reconciled, conformance corpus
completeness audited).

---

## RFC candidates from the OPEN matrix

Amendments most likely to need RFC treatment before v1.1+ promotion:

- **A11** — first-class union ADTs (replace the `Result[T, Error]`
  sentinel lowering).
- **A47** — Wasm Component Model wrappers (full `wit-component`).
- **A94** — sandboxed proc-macro execution surface.
- **A100** — per-call FsCap materialisation from sandbox manifest at
  the MtyIR lower.
- **A102** — agent affinity front-end syntax design.
- **A103** — lossless live agent migration design.

The OPEN-status amendments without architectural questions (A45
deprecation, A49 monomorphisation expansion, A80 labelled break, A81
trait-based iterators, A92 set-of-scopes hygiene, A93/A95 mty-pkg
wiring) are tracked as ordinary backlog work rather than RFCs.
