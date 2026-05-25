# v0.8 Spec Consolidation Notes (Mighty v1.0-RC)

This document is the working notebook for the v0.8 spec consolidation
slice. It records the reconciliation decisions, interpretation calls,
and post-v0.8 flags raised while folding 88 amendments into the v1.0
release candidate spec ([`docs/spec/v1.0-rc.md`](docs/spec/v1.0-rc.md)).

**Slice owner:** spec consolidation swarm agent.
**HEAD at start:** `36b3140`.
**Branch:** `main` (single-branch workflow per autonomous build mandate).

---

## Output files

| Path                                     | Purpose                                  |
|------------------------------------------|------------------------------------------|
| `docs/spec/v1.0-rc.md` (NEW)             | normative v1.0 release candidate spec    |
| `docs/spec/v0.1-amendments.md` (EDITED)  | added `**Status:**` line per amendment + legend |
| `docs/spec/CHANGELOG.md` (NEW)           | chronological log per ladder step        |
| `SPEC_CONSOLIDATION_V0_8_NOTES.md` (NEW) | this file                                |
| `scripts/classify_amendments.py` (NEW)   | reproducible status-line injector        |

No crate source files were touched. No `Cargo.toml` was touched. No
behavioural test was modified.

---

## Classification result

88 amendments analysed end-to-end:

| Status     | Count |
|------------|-------|
| FROZEN     | 63    |
| SUPERSEDED | 15    |
| OPEN       | 10    |
| REVERTED   | 0     |

Numbering gaps (no amendment shipped under these IDs):

- A57..A64 — reserved during v0.3 drafting; no published amendment.
- A66..A69 — reserved during v0.3 drafting; no published amendment.
- A75..A79 — reserved during v0.5 drafting; no published amendment.
- A83..A89 — reserved during v0.5 drafting; no published amendment.

This matches the 88 published headers in `docs/spec/v0.1-amendments.md`
(`grep -c "^## A" = 88`).

---

## Reconciliations (contradictions resolved during consolidation)

### R1 — Copy set: A13 vs A26

**A13 (slice 4):** hardcoded a Copy set including all primitives, refs,
function pointers, Str, tuples/arrays of Copy elements, and **opaque
prelude ADTs**.

**A26 (slice 5):** introduced `#[derive(Copy)]` with per-type field
validation.

**Conflict:** A13 admitted opaque prelude ADTs as blanket-Copy; A26's
field-level validation would reject most of them.

**v1.0-RC resolution (§7.1):** Copy set is the union of:

- `#[derive(Copy)]` user types whose every field is itself Copy
  (A26 enforced via MT4040).
- Primitives, refs, function pointers, Str, tuples/arrays of Copy
  elements (A13 base set).
- Opaque prelude ADTs (A13 blanket — explicitly carried into v1.0 to
  keep canonical examples ergonomic).

The opaque-prelude blanket is **flagged as a v1.0 carryover that the
v1.1+ typed-dispatch pass should refactor away** (no spec amendment
written for the carryover; tracked in this NOTES file under R10).

### R2 — Sendable membership: A14 vs A65.b

**A14 (slice 4):** conservative Sendable definition treating opaque
ADTs as Sendable permissively, and `Param`/`Var` parameters as
Sendable.

**A65.b (v0.3):** formal Sendable definition with explicit
non-Sendable classification of capability handles, `dyn Trait`,
references, and any transitively containing compound.

**Conflict:** A14 admitted compounds that A65.b explicitly forbids.

**v1.0-RC resolution (§7.8):** A65.b's definition is normative; A14 is
SUPERSEDED. Generic / unbound types are treated permissively at the
check site (the only A14 idea that survives — and only because it's
sound: monomorphic call sites perform the real check downstream).

### R3 — Borrow region semantics: A20 vs A55

**A20 (slice 4):** lexical region (Rust 2015 style) — borrow ends at
end of enclosing block.

**A55 (v0.3):** NLL last-use deactivation — borrow ends at last use of
the borrower binding in source order.

**Conflict:** A20 over-rejects programs A55 admits. A55 is strictly
more permissive.

**v1.0-RC resolution (§7.4):** A55 is normative. A20 is SUPERSEDED.
The v1.0 NLL implementation is documented as a hand-rolled
approximation (not Polonius) with three explicit gaps: two-phase
borrows, branch-divergent borrows on a diamond, loop back-edge
borrows beyond the fixed-point detection (A82). The gaps are listed
in §7.4 as v1.0 OPEN.

### R4 — Scope tolerance policy: A21 vs A65

**A21 (slice 4):** unresolved single-segment value names resolve to
fresh inference variables unconditionally (with a tolerance set
escape).

**A65 (v0.3):** scope-aware permissive/strict split — only permissive
scopes get the fresh-var fallback; strict scopes (agent, handler) emit
MT2021.

**Conflict:** A21 is unconditional; A65 conditionalises by scope.

**v1.0-RC resolution (§5.1):** A65 is normative. A21 is SUPERSEDED.
The supervisor and cap-narrow bodies are marked strict-but-open in
v1.0 (the `tolerance_open=true` toggle), preserving slice-7 surface
compatibility while activating MT2021 automatically once the runtime
gains first-class supervisor bindings.

### R5 — Method dispatch policy: A10 vs A17

**A10 (slice 3):** built-in method table is permissive (variadic,
fresh-var return) for every receiver, including user ADTs.

**A17 (slice 4):** user ADTs require `impl` blocks; unknown methods on
a user ADT emit MT2007.

**Conflict:** A10 admits methods A17 rejects on user ADTs.

**v1.0-RC resolution (§19.3):** A17 is normative for user ADTs. A10
is SUPERSEDED for user ADTs but **survives residually** for opaque
prelude ADTs and primitives so canonical examples that call arbitrary
methods on opaque receivers (`url.parse(...)`) continue to compile.
The residual permissive table is explicitly listed as **v1.0 OPEN —
v1.1+ removal** in §19.3 and Appendix A.

### R6 — Sandbox enforcement model: A27 vs A34 vs A43

**A27 (slice 5):** top-level `sandbox` items parse and lower as
`HirItem::Sandbox`; type-check tolerates body under sandbox tolerance.
Runtime semantics deferred.

**A34 (slice 6):** budgets + sandboxes are metadata in the
interpreter — the body emits unchanged, entries are not enforced.

**A43 (slice 7):** runtime execution lands — fresh `BudgetTracker` per
sandbox, capability calls checked against allowlists, nested sandboxes
compose via intersection.

**v1.0-RC resolution (§16.1):** A43 is normative. A27 and A34 are
SUPERSEDED. The slice-progression chain is documented for traceability
but no longer affects v1.0 readers.

### R7 — Cancellation timing: A41 vs A70

**A41 (slice 7):** deadline arrives at the next await point; cannot
pre-empt a running turn; cancels the next queued turn. Acceptable
because every turn is bounded by an interpreter step budget.

**A70 (v0.3):** cooperative mid-turn cancellation via
`tokio::task::spawn_blocking` + `CancellationToken`. The blocking
thread is detached on cancel; reply notification is exactly-once.

**Conflict:** A41 cancels between turns; A70 cancels mid-turn.

**v1.0-RC resolution (§15.2):** A70 is normative. A41 is SUPERSEDED.
The detached-thread + step-budget bound from A41 survives as the
worst-case wall-time guarantee for runaway handlers (the async parent
doesn't wait for the detached thread; the interpreter's 1M-step budget
caps the thread's lifetime).

### R8 — Telemetry wire format: A38 vs A71

**A38 (slice 7):** OpenTelemetry-flavoured JSON, one event per line on
stderr. Strict OTLP deferred.

**A71 (v0.3):** real `opentelemetry_sdk::TracerProvider` with OTLP
exporter over gRPC. Activation is env-driven and best-effort; OTLP
failure falls through to the JSON sink without breaking runtime
construction.

**Conflict:** A38 says "JSON only"; A71 says "OTLP when endpoint
configured".

**v1.0-RC resolution (§35):** Both wire formats coexist normatively.
A38's JSON-line sink is the default. A71's OTLP exporter activates on
`STARDUST_OTLP_ENDPOINT=<url>` and falls back to the JSON sink on
failure. A38 is reclassified as "default sink"; A71 layers on top as
the opt-in real-OTLP path. Documented as parallel transports rather
than supersession.

### R9 — Memory budget enforcement: A37 vs A50 vs A99

**A37 (slice 7):** approximate memory budget via explicit
`BudgetTracker::record_mem(n)` calls. Interpreter does not auto-count.

**A50 (slice 8):** `bumpalo`-backed arenas with byte-counting against
`BudgetTracker::mem_bytes` for codegen-cranelift path.

**A99 (v0.5 dogfood):** interpreter auto-charges `AdtInit`,
`TupleInit`, `ArrayInit` via `estimate_value_bytes`; new
`RunResult::MemBudgetExceeded` outcome.

**Conflict:** A37 says approximate-only; A50 says real-arena; A99 says
auto-charge in interp.

**v1.0-RC resolution (§10.4):** A37 is SUPERSEDED by the combination
A50 + A99. A50 covers the arena allocator path; A99 covers the
interpreter value-init path. Together they provide real, end-to-end
memory budget enforcement. The slice-6 interpreter's byte-counter
survives behind `--legacy-interp` (A45).

### R10 — Opaque-ADT carryover (no published amendment; identified here)

The v1.0-RC §7.1 Copy set includes "Opaque prelude ADTs" as a blanket
admission for ergonomic continuity with canonical examples. This is a
v1.0 carryover decision, not a normative invariant; the v1.1+ typed
inherent/trait dispatch pass should refactor the prelude so each
opaque ADT either explicitly derives Copy or doesn't.

**Action:** flagged in §7.1 and Appendix A.2 as residual; no RFC
needed (it's a code-side cleanup, not a spec-level decision change).

### R11 — Default worker count: A39 vs A106

**A39 (slice 7):** `RuntimeBuilder::deterministic(seed)` swaps tokio
executor for current-thread runtime. Default worker count is 1 (single
tokio runtime).

**A106 (v0.6):** Default worker count switches to
`available_parallelism()`. Deterministic mode still forces a single
worker for reproducibility.

**Conflict:** A39's "default 1" vs A106's "default available_parallelism".

**v1.0-RC resolution (§25.5):** A106 is normative for the default;
A39's single-worker pin survives **only** under `deterministic(seed)`.
A39 is reclassified as the deterministic-mode contract; A106 is the
default-mode contract. Documented as complementary in §25.5 and
Appendix A.

### R12 — `mty run` default execution path: A45 vs A48

**A45 (slice 7):** `mty run` defaults to the slice-7 runtime path;
`--legacy-interp` invokes the slice-6 synchronous interpreter.

**A48 (slice 8):** `mty run` defaults to JIT (Cranelift) and falls
back to the slice-7 runtime on `CodegenError::Unsupported`.

**Conflict:** A45 says default = slice-7 runtime; A48 says default =
JIT-then-runtime.

**v1.0-RC resolution (§24.3):** A48 is normative. A45's
`--legacy-interp` flag survives but is **v1.0 OPEN — deprecation
review v1.1+**. The fallback chain is documented as JIT → slice-7
runtime → (`--legacy-interp` opt-in) slice-6 interpreter.

---

## Interpretation calls (non-contradiction decisions)

### I1 — Spec was a stub on disk; reconstructed from amendments + internals

The in-repo `docs/spec/v0.1.md` is a 73-line stub that references an
external `mighty_language_spec_v0_1.md`. The original ~1879-line spec
file was not present on disk at consolidation time (the project
owner's working copy lives outside the repo). The v1.0-RC was
reconstructed from:

- The 88-amendment narrative in `docs/spec/v0.1-amendments.md`.
- The 38 internals docs under `docs/internals/*.md`.
- The release notes `RELEASE-v0.X.md` for slices 1..6.
- The slice docs `SLICE*.md` for the per-slice scope.
- The rebrand log `REBRAND_NOTES.md`.

This means the v1.0-RC **inherits the structure of the original 39-section
spec by inference**, not by direct port. Spec sections that the
amendments did not touch (e.g. the deeper parser productions, the exact
HIR/AIR mapping tables, the C++ ABI specifics) are summarised at the
conceptual level rather than reproduced normatively.

**Action:** when the original `mighty_language_spec_v0_1.md` becomes
available, a v1.0-RC.1 docs-pass should reconcile any normative
specifics the amendments did not cover. Flagged for post-v0.8.

### I2 — Section numbering preserved as 1..39 with insertions

The original spec had 39 sections. v1.0-RC keeps the same 1..39
numbering with new sections inserted in logical order. New sections
gained between v0.1 and v1.0:

- §30 Profiles (originally a sub-section under §9 Effects / §16 Budgets)
- §35 Telemetry and observability (originally sprinkled across §25 Runtime)
- §36 Package manager and registry (originally absent — package
  manager landed in v0.4)
- §37 LSP and editor integration (originally absent)
- §38 Benchmarks and performance budgets (originally absent — bench
  corpus landed in v0.6)
- §39 Self-hosting (originally absent — lexer/parser self-host landed
  v0.5/v0.6)

The §31 Construction history and §32 Conformance suite sections from
the original spec are retained.

### I3 — `STARDUST_*` env-var prefix preserved post-rebrand

The rebrand sweep intentionally retained the `STARDUST_*` env-var
prefix for back-compat with v0.6 deployments. v1.0-RC documents the
env vars under their original names (§29.3); a `MIGHTY_*` alias may
be added in v1.1+ but is not in scope for v1.0.

### I4 — `pkg.stardust.dev` registry URL retained

Per REBRAND_NOTES.md follow-up #1, the default registry URL is still
`pkg.stardust.dev`. v1.0-RC §36.2 documents this and notes the
swap path if `pkg.mighty.dev` becomes available.

### I5 — Conformance corpus counts are placeholders

§32 lists per-category case counts. Several counts are marked `TBD`
because the consolidation pass did not re-walk the corpus directory.
The v0.6 release notes report 885 tests passing overall; per-category
breakdowns require re-running `cargo test --list` per package.

**Action:** flagged for post-v0.8 docs-pass to fill in the exact
per-category numbers.

### I6 — RFC candidates separated from ordinary OPEN amendments

The OPEN matrix in `v1.0-rc.md` Appendix A.2 is unified, but the
CHANGELOG.md flags a subset as **RFC candidates** (A11, A47, A94,
A100, A102, A103). These are the amendments whose v1.1+ evolution
needs architectural design discussion before implementation. The
remainder (A45, A49, A80, A81, A92, A93, A95) are ordinary backlog
work that can be picked up without RFC ceremony.

### I7 — Status counts in legend vs amendments

The amendments file legend (auto-generated by
`scripts/classify_amendments.py`) reports 63 FROZEN / 15 SUPERSEDED /
10 OPEN / 0 REVERTED = 88. The cover memo in this NOTES file matches
those counts. If a future consolidation pass reclassifies any
amendment (e.g. promotes an OPEN to FROZEN once the v1.1+ work
lands), the script is the single source of truth — re-run it and the
legend updates automatically.

---

## Post-v0.8 follow-ups

In addition to the v1.1+ OPEN-amendment roadmap, the consolidation
pass identified follow-up work specific to the spec docs:

### F1 — Reconcile against original v0.1 spec when available

See I1. When the project owner's external
`mighty_language_spec_v0_1.md` becomes available, a docs-pass should
verify every normative claim in v1.0-RC against the original 39-section
text. Any divergence should be either:

- recorded as an additional amendment in v0.1-amendments.md, OR
- reconciled in this NOTES file under a new R-numbered entry.

### F2 — Fill conformance counts (I5)

§32 has several `TBD` cells. Run `cargo test --list -p mty-types`,
`-p mty-borrow`, etc. and update the per-category breakdown.

### F3 — Refresh worked example list against examples/ dir

§34 lists 20 worked examples by inferred filename pattern (`01_hello.mty`,
`02_types.mty`, ...). The exact filename and per-example LOC may have
drifted across v0.4 dogfood and v0.7 rebrand. Cross-check
`ls examples/` and update the table.

### F4 — Promote v1.0-RC to v1.0 stable

After the v1.0-RC bake-in period (TBD by project owner), v1.0 stable
locks the FROZEN matrix. The v1.0.0 tag should:

- Move `docs/spec/v1.0-rc.md` to `docs/spec/v1.0.md` (or symlink).
- Open a `docs/spec/v1.1-amendments.md` file for the next ladder
  step's amendments.
- Update README spec links.

### F5 — RFC process design

Open a `docs/rfcs/` directory and draft a one-page RFC template. The
RFC candidates listed in CHANGELOG.md are the first six issues to
file.

### F6 — Migrate STARDUST_ env vars to MIGHTY_ aliases (v1.1+)

Add `MIGHTY_RUNTIME_THREADS`, `MIGHTY_OTLP_ENDPOINT`, `MIGHTY_TRACE`,
`MIGHTY_LINKER` as aliases of the corresponding `STARDUST_*` vars
under §29.3. Keep `STARDUST_*` recognised through v1.x.

### F7 — Doc cross-link sweep

The v1.0-RC links to many `docs/internals/*.md` and `RELEASE-v0.X.md`
files. A docs-pass should verify each link resolves; the file paths
were stable through the v0.7 rebrand but a final check is cheap.

---

## Verification

All work in this slice is docs-only. No crate sources were touched,
no `Cargo.toml` was touched, no test was modified. The pre-existing
`cargo test --workspace` corpus (885 tests passing per v0.6 release
notes) is unchanged.

To verify the consolidation pass:

```bash
# Status legend present and accurate
grep -c "^\*\*Status:\*\*" docs/spec/v0.1-amendments.md
# expect: 88

# All amendment headers covered
grep -c "^## A[0-9]" docs/spec/v0.1-amendments.md
# expect: 88

# v1.0-RC self-contained
wc -l docs/spec/v1.0-rc.md
# expect: 2500+

# Re-run classifier (idempotent)
python scripts/classify_amendments.py
# expect: FROZEN: 63, SUPERSEDED: 15, OPEN: 10, REVERTED: 0
```

---

## Report

**Status:** SHIPPED (spec consolidation slice).

- **Files added:** `docs/spec/v1.0-rc.md`, `docs/spec/CHANGELOG.md`,
  `SPEC_CONSOLIDATION_V0_8_NOTES.md`, `scripts/classify_amendments.py`.
- **Files modified:** `docs/spec/v0.1-amendments.md` (status lines + legend).
- **Amendments classified:** 88 total (63 FROZEN / 15 SUPERSEDED / 10
  OPEN / 0 REVERTED).
- **v1.0-RC line count:** ~2520 lines.
- **Reconciliations recorded:** 12 (R1..R12).
- **Interpretation calls recorded:** 7 (I1..I7).
- **Post-v0.8 follow-ups flagged:** 7 (F1..F7).
- **RFC candidates flagged:** 6 (A11, A47, A94, A100, A102, A103).
- **Tests touched:** 0 (docs-only).
- **Crate sources touched:** 0.
