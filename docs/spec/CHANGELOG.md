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
[`SPEC_CONSOLIDATION_V0_8_NOTES.md`](../../SPEC_CONSOLIDATION_V0_8_NOTES.md).

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

See [`RELEASE-v0.4.md`](../../RELEASE-v0.4.md) and
[`REGISTRY_V0_4_NOTES.md`](../../REGISTRY_V0_4_NOTES.md).

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

Preserved:

- GitHub repo URL `hassard0/stardust` (per project owner decision)
- `edition = "2026"` (calendar year, not brand)
- `wasi:*` WIT namespace (upstream WASI types)
- `STARDUST_*` env var prefix (back-compat with v0.6 deployments)
- `pkg.stardust.dev` registry URL constant (until a Mighty-branded
  registry comes online)

See [`REBRAND_NOTES.md`](../../REBRAND_NOTES.md) and
[`RENAME_LOG.md`](../../RENAME_LOG.md) for the full interpretation
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
- [`SPEC_CONSOLIDATION_V0_8_NOTES.md`](../../SPEC_CONSOLIDATION_V0_8_NOTES.md) —
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

## v1.0 (planned) — release stability

After the v1.0-RC bake-in period, v1.0 stable ships with:

- The FROZEN matrix locked.
- The OPEN matrix continues to evolve in v1.x minor releases per
  [Appendix B](v1.0-rc.md#appendix-b--backwards-compatibility-policy).
- A v1.1 RFC process opens for any of the OPEN amendments that the
  community wants to promote.

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
