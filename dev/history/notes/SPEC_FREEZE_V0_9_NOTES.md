# Spec Freeze v0.9 Notes (Mighty v1.0-RC2)

This document is the working notebook for the v0.9 spec-freeze
preparation slice. It records:

- Per-amendment resolutions for the 10 OPEN amendments left by v0.8.
- The six RFCs drafted under `docs/spec/rfcs/`.
- The v1.0-RC2 promotions and appendices added to
  [`docs/spec/v1.0-rc.md`](docs/spec/v1.0-rc.md).
- The remaining v1.0 stable-release blockers and proposed freeze date.

**Slice owner:** spec-freeze swarm agent.
**HEAD at start:** `808770e`.
**Branch:** `main` (single-branch workflow per autonomous build mandate).

---

## Output files

| Path                                          | Purpose                                       |
|-----------------------------------------------|-----------------------------------------------|
| `docs/spec/rfcs/RFC-001-first-class-union-adts.md` (NEW)             | A11 follow-up RFC          |
| `docs/spec/rfcs/RFC-002-wasm-component-model-wrapper.md` (NEW)       | A47 + A97 follow-up RFC    |
| `docs/spec/rfcs/RFC-003-sandboxed-proc-macro-execution.md` (NEW)     | A94 follow-up RFC          |
| `docs/spec/rfcs/RFC-004-per-call-fscap-manifest.md` (NEW)            | A100 follow-up RFC         |
| `docs/spec/rfcs/RFC-005-affinity-frontend-syntax.md` (NEW)           | A102 follow-up RFC         |
| `docs/spec/rfcs/RFC-006-lossless-live-agent-migration.md` (NEW)      | A103 follow-up RFC         |
| `docs/spec/v1.0-rc.md` (EDITED)               | promoted to v1.0-RC2; appendices added        |
| `docs/spec/v0.1-amendments.md` (EDITED)       | Status lines updated for 10 OPEN amendments   |
| `docs/spec/CHANGELOG.md` (EDITED)             | v1.0-RC2 entry added                          |
| `SPEC_FREEZE_V0_9_NOTES.md` (NEW)             | this file                                     |

No crate source files were touched. No `Cargo.toml` was touched. No
behavioural test was modified.

---

## OPEN-amendment resolutions

The v0.8 consolidation classified 10 amendments as **OPEN**. v0.9
resolves each one with **FREEZE-MVP**, **DEFER-V1.1**, or **DROP**.

| Amendment | v0.8 status | v0.9 resolution | RFC      |
|-----------|-------------|-----------------|----------|
| A11       | OPEN        | DEFER-V1.1      | RFC-001  |
| A15       | OPEN        | FREEZE-MVP      | —        |
| A31       | OPEN        | FREEZE-MVP      | —        |
| A45       | OPEN        | DEFER-V1.1      | —        |
| A47       | OPEN        | DEFER-V1.1      | RFC-002  |
| A49       | OPEN        | FREEZE-MVP      | —        |
| A94       | OPEN        | DEFER-V1.1      | RFC-003  |
| A97       | OPEN        | DEFER-V1.1      | RFC-002  |
| A102      | OPEN        | DEFER-V1.1      | RFC-005  |
| A103      | OPEN        | DEFER-V1.1      | RFC-006  |

Note on A100. The v0.8 consolidation classified A100 as FROZEN, but
its v0.5 status line carried a residual "per-call materialisation from
manifest still v1.1+" tag, and the v0.8 CHANGELOG flagged it as an
RFC candidate. v0.9 keeps A100 FROZEN (the process-wide-default cap
enforcement is the v1.0 normative contract) but ships
[RFC-004](docs/spec/rfcs/RFC-004-per-call-fscap-manifest.md) for the
v1.1+ MtyIR-lower threading work flagged in A100's residual tag and
in A109's per-call isolation invariant.

Resolution semantics:

- **FREEZE-MVP.** The amendment's MVP behaviour is stable enough to be
  part of the v1.0 normative contract. The v1.0 stability surface
  guarantees the MVP's behaviour; any v1.x expansion lands as a new,
  additive amendment that does not change MVP semantics.
- **DEFER-V1.1.** The amendment ships in v1.0 with its current
  behaviour but its design is not yet final. The v1.0 contract for
  these features is documented in §A.2 of v1.0-RC2; an RFC drives the
  v1.1+ promotion. v1.0 → v1.1 may introduce backwards-incompatible
  behaviour within the OPEN feature's scope.
- **DROP.** The amendment is no longer relevant; cross-reference its
  superseder. (None in v0.9.)

### A11 — Anonymous error unions → DEFER-V1.1

The slice-3 sentinel lowering (`T!{A, B}` → `Result[T, Error]` with a
poison `Error`) is the v1.0 normative contract. It is a stopgap, but
it ships unchanged in v1.0 because the real fix — first-class union
ADTs — is non-trivial and demands an RFC-driven design (see
[`RFC-001`](docs/spec/rfcs/RFC-001-first-class-union-adts.md)).

The v0.8 consolidation already documented A11 in §A.2 of v1.0-RC. v0.9
adds the RFC cross-reference and explicit "DEFER-V1.1" wording in both
amendments doc and §A.2.

**v1.0 contract.** Anonymous error unions resolve to `Result[T, Error]`.
The `Error` type unifies permissively. Exhaustiveness checks against
the union variants do not fire MT2015 in v1.0.

**v1.1+ promotion path.** RFC-001 adopts a 30-day comment window;
post-acceptance the typeck adds `TyData::Union`, the desugar pass
interns anonymous unions, MT2015 activates against the desugared
variant set. Migration guidance ships with v1.1-alpha.

### A15 — Arena escape direct-naming MVP → FREEZE-MVP

The v0.4 borrow-check `MT3010` arena-escape detector covers the
**direct-naming** case (an arena body's tail expression names an
arena-local non-Copy binding). Indirect flow (a fn that captures and
returns the arena-local value) is unreachable at v0.4..v0.6 but
covered defensively by `MT3010`-equivalent runtime trap `MT5007`,
which v0.7 reserves but does not emit.

For v1.0, the **direct-naming MVP** is the normative contract. It is
sound: it never lets a value escape with a borrow tied to a dead arena
scope. The indirect-flow gap is a *completeness* gap (some programs
that should be safe are statically rejected), not a *soundness* gap.

**FREEZE-MVP decision.** Direct-naming `MT3010` is v1.0 FROZEN. The
v1.1+ indirect-flow detector ships as a new amendment (provisionally
A110) and is purely *additive*: it never rejects a program v1.0
accepts; it only accepts more programs. This means freezing the MVP
costs nothing — programs that compile in v1.0 continue to compile in
v1.x.

The runtime `MT5007` trap stays reserved but unimplemented in v1.0.
v1.1+ wires it on the runtime side; v1.0 already statically guards
the direct-naming case.

### A31 — Arena runtime enforcement → FREEZE-MVP

A31 originally noted "arena runtime enforcement deferred" with
`MT5007 arena_escape_runtime` reserved for a future trap. v0.8
classified it OPEN. Reviewing the v0.5..v0.6 state, the *practical*
arena enforcement at v1.0 is the combination of:

- Static `MT3010` borrow-check direct-naming detection (A15).
- `bumpalo`-backed arena allocator with byte-charging (A50).
- Interpreter auto-charging via `estimate_value_bytes` (A99).

This combination is **complete for every program v0.5..v0.7 dogfood
exercises**, including the http server and the LSP-driven editor
load. The only gap is the indirect-flow case (a fn that captures and
returns an arena-local) — and A15's static check rejects every such
program at compile time, so the runtime trap MT5007 has never been
reached in dogfood.

**FREEZE-MVP decision.** The static + bumpalo + auto-charging combo
is the v1.0 normative arena runtime contract. MT5007 stays *reserved*
in v1.0 for a future v1.1+ activation that pairs with the indirect-
flow detector (the additive A110-provisional from A15's promotion
path). Because v1.0 already statically rules out the only flow MT5007
would trap, freezing the MVP costs nothing.

### A45 — `mty run --legacy-interp` opt-out → DEFER-V1.1

The flag retains diagnostic value: a developer comparing slice-7
runtime behaviour against the slice-6 interpreter for a suspected
runtime regression uses `--legacy-interp` as the comparison oracle.
Removal premature in v1.0 because the v1.0 Cranelift backend does not
yet cover the full MtyIR surface (A49 strips generics; the runtime
fallback is exercised by every generic-heavy program).

**DEFER-V1.1.** Flag retained; deprecation review at v1.1 once
RFC-future (monomorphisation) closes the codegen coverage gap. Until
then, `--legacy-interp` continues to route through
`pipeline::run_file` exactly as in v0.7.

**v1.0 contract.** `mty run <file>` defaults to JIT → slice-7 runtime
fallback chain (per A48). `--legacy-interp` routes through the slice-6
synchronous interpreter. Both produce the same observable behaviour
modulo deterministic-mode replay.

### A47 — Wasm Component Model wrapper deferred → DEFER-V1.1

The v0.8 core-module output is the v1.0 normative contract. Capability
imports are declared as plain function imports under the `mighty`
module namespace. The full `wit-component` wrapper + WIT auto-binding
+ preview2 / `wasi-cli` integration ships in v1.1+ under
[`RFC-002`](docs/spec/rfcs/RFC-002-wasm-component-model-wrapper.md).

**v1.0 contract.** `mty build --target wasm32-web` emits a core
module. The hand-written WIT sketch at
`docs/internals/codegen-wasm.md` documents the eventual shape but is
not normative at v1.0.

**v1.1+ promotion path.** RFC-002 adopts a 60-day comment window. The
WIT files under `crates/mty-codegen-wasm/wit/` are promoted to
`docs/spec/wit/v1.1/` and become normative. The component-output
mode lands behind `--target wasm32-component` opt-in flag in
v1.1-alpha and becomes default in v1.1-beta.

### A49 — Per-(fn, type-args) monomorphisation strips generics → FREEZE-MVP

The v0.8 MVP strips generic fns from the codegen unit; programs that
exercise generics route through the interpreter fallback (a clean
A48 fallthrough). This is sound and stable: it never miscompiles, it
just trades performance for simplicity.

**FREEZE-MVP decision.** The strip-MVP behaviour is the v1.0 normative
contract. The v1.1+ full specialisation ships as a purely additive
optimisation: programs that worked via the interpreter fallback in
v1.0 continue to work, but now via the Cranelift path. No source-level
change required for users.

**v1.0 contract.** Generic fns:
- Type-check normally.
- Are stripped from the codegen unit at the monomorphizer pass.
- Execute via `pipeline::run_file_with_runtime` (the slice-7
  interpreter fallback per A48).

**v1.1+ promotion path.** A new amendment (provisionally A111) lands
per-(fn, type-args) specialisation. The interpreter fallback remains
in place as a safety net. No RFC required — the design is settled;
the work is implementation.

### A94 — Procedural macros parse + store → DEFER-V1.1

The v0.8 contract is "parse + store + purity check + MT6006 at every
call site". This is intentionally a gate: it keeps source survival
(declarations and call sites both round-trip through the parser
unchanged) so v1.1+ can lift the gate without re-parsing every Mighty
source.

**v1.0 contract.** `proc macro Name(input: TokenStream) -> TokenStream
{ body }` parses; the body stores as opaque tokens. `MT6005` fires at
decl time if the body references an impure surface (`time`, `env`,
`io`, `model`, `rand`). `MT6006` fires at every call site and replaces
the call with the sentinel literal `0`.

**v1.1+ promotion path.** [`RFC-003`](docs/spec/rfcs/RFC-003-sandboxed-proc-macro-execution.md)
adopts a 30-day comment window. The sub-interpreter, TokenStream
marshalling protocol, sandbox capability model, and determinism
contract are detailed there. v1.1-alpha ships behind
`--experimental-proc-macro` flag; the flag flips at v1.1-beta.

### A97 — `mighty:web/dom` interface added → DEFER-V1.1

The v0.8 contract is "interface declared, four-method surface
(`set-text` / `get-text` / `on-click` / `query`) ships in the core
module's WIT sketch, canonical-ABI return-area bridge for
`option<string>` / `string` returns deferred". v0.5 back-compat
imports (`get-element-by-id` / `set-text-handle`) remain.

**v1.0 contract.** The four DOM methods are declared in the WIT
sketch. `set-text` and `on-click` have full canonical-ABI support
(both take `string` / `string` and return `()`); `get-text` and
`query` return `u32` handles in v1.0 with a separate `read-string-
handle(u32) -> string` shim provided by the browser embed.

**v1.1+ promotion path.** Covered by RFC-002. The component-model
canonical ABI's return-area protocol lands during the v1.1-alpha
component-output work; `get-text` and `query` switch to real
`option<string>` returns. The v0.5 handle-based back-compat imports
remain in `mty:web-compat@1.0.0` for one major cycle.

### A100 carryover — FsCap per-call manifest materialisation (v1.0 status: FROZEN; RFC-004 tracks v1.1+)

A100 itself is **FROZEN** at the v0.8 consolidation: the v0.5 dogfood
process-wide-default cap enforcement is the v1.0 normative contract,
and `host::dispatch` consults the current default cap on every
`std.fs.*` call. The v0.5 status line carried a residual "per-call
materialisation from manifest still v1.1+" tag, and the v0.8
CHANGELOG flagged A100 as an RFC candidate for that residual gap.

A109 (also FROZEN) added the per-call isolation invariant test
proving two `FsCap` values with disjoint allowlists never leak
across the divide. The MtyIR-lower threading that would replace
the process-wide default with explicit per-call cap arguments is
the remaining gap that [`RFC-004`](docs/spec/rfcs/RFC-004-per-call-fscap-manifest.md)
covers. It is a v1.1+ work item, not a v0.9 resolution.

**v1.0 contract (unchanged from v0.5/v0.6).**
`std.fs.{read, write, exists, list_dir}` accept a `&FsCap` per
call. The `host::dispatch` consults the current process-wide
default cap on each call; a `Forbidden` path returns
`Result::Err(forbidden:<path>)`. Per-call materialisation from
sandbox manifests at the MtyIR lower is **NOT** the v1.0 contract;
hosts must `install_default_*_cap` once per process.

**v1.1+ promotion path.** RFC-004 adopts a 30-day comment window.
The MtyIR shape gains a cap-arg prefix to every `Fs*` call; the
lowerer materialises the cap at sandbox entry. The process-wide
default cap is retained for unsandboxed call sites and v1.0 host
back-compat.

### A102 — Agent affinity hints → DEFER-V1.1

The v0.6 runtime API (`Affinity::Sticky` / `Elastic` +
`RuntimeBuilder::spawn_agent_with_affinity`) is frozen. The front-end
syntax `agent X(...): Y with affinity = sticky` is reserved-but-not-
parsed in v1.0.

**v1.0 contract.** The runtime API is the only way to express affinity
at v1.0. Source-level `with affinity = ...` is reserved (the parser
will not parse it; emitting an MTxxxx-coded "syntax not yet supported"
error if attempted is **future** work).

**v1.1+ promotion path.** [`RFC-005`](docs/spec/rfcs/RFC-005-affinity-frontend-syntax.md)
adopts a 14-day comment window. The grammar lands in v1.1-alpha;
LSP semantic-token + hover support follows; supervisor inheritance
lands in v1.1-beta. Conformance corpus gains an `affinity/` category.

### A103 — Lossless live migration → DEFER-V1.1 (possibly DEFER-V1.2)

The v0.6 lightweight migration (routing-table-only) is the v1.0
normative contract. Lossless live migration of in-flight agents is
non-trivial because tokio doesn't expose per-task waker-set re-binding;
the v0.7+ scoping reflected this.

**v1.0 contract.** `LoadMonitor` updates the routing table so the
**next** spawn of an agent lands on a lighter worker. Existing loops
continue on their original worker. This is enough for steady-state
load balancing; it is **not** sufficient for drain-for-shutdown,
hot-spot relief, or live upgrades.

**v1.1+ promotion path.** [`RFC-006`](docs/spec/rfcs/RFC-006-lossless-live-agent-migration.md)
adopts a 60-day comment window. The RFC ships a first-draft design
but flags the Detailed Design as draft-pending-owner: the runtime
primitives (tokio waker-set re-binding, mailbox snapshot/restore,
cancellation-token transfer) are open implementation questions. If
they prove harder than the v1.1-alpha cycle can absorb, the RFC
slips to v1.2 and v1.1 ships only the `#[derive(Migratable)]`
surface.

---

## v1.0-RC2 changes

### Title bump

v1.0-RC → v1.0-RC2 in the document title and the `**Status:**` line.
RC2 is dated 2026-05-24 (v0.9 freeze-prep slice).

### New appendix: v1.0 stability surface (Appendix A.1 promoted)

The existing §A.1 FROZEN features list already serves as the
stability surface. v0.9 promotes its prose:

> The v1.0 stability surface is the set of FROZEN features in §A.1.
> These features will not break in any v1.x release. New FROZEN
> features may be added (additive only); existing FROZEN features
> may have their messages refined but not their fire conditions.

### v1.1 promotion targets

The existing §A.2 OPEN features list is repurposed as the v1.1
promotion target backlog. Each entry now cross-references its RFC or
ordinary-backlog tracker.

### RFC cross-references

§A.2 gains an RFC column or inline link per OPEN entry. The cross-
reference table at the end of v1.0-RC2 (Appendix C) is extended with
a "RFCs by amendment" subsection.

### CHANGELOG

A new v1.0-RC2 section in `docs/spec/CHANGELOG.md` lists all
resolutions and RFCs.

---

## v1.0 freeze plan

### Path from v0.9 to v1.0 stable

| Step | Owner       | Trigger                                    |
|------|-------------|--------------------------------------------|
| 1    | v0.9 slice  | Resolve 10 OPEN amendments + draft RFCs (this slice). |
| 2    | v0.10 slice | RFC polish: open public comment windows, recruit design owners for RFC-006. |
| 3    | v0.11 slice | First independent re-implementation attempt: at least one external party builds a Mighty-conformant lexer + parser + typeck against v1.0-RC2 spec. |
| 4    | v0.12 slice | Conformance corpus completeness pass: every FROZEN feature in §A.1 has at least one conformance test; gaps filled. |
| 5    | v1.0 freeze | 30-day no-substantive-change window on v1.0-RC2. |
| 6    | v1.0.0 tag  | Promote `docs/spec/v1.0-rc.md` → `docs/spec/v1.0.md`; lock FROZEN matrix. |

### v1.0 freeze blockers

In priority order:

1. **30-day RFC comment period** — each of the 6 RFCs needs a public
   comment window before v1.1 promotion. v1.0 *itself* doesn't need
   the RFCs accepted (the OPEN amendments stay OPEN until v1.1), but
   the spec freeze requires that the v1.0 → v1.1 evolution path be
   clearly documented. RFCs as drafted in v0.9 satisfy the
   documentation requirement; comment-period acceptance is a v1.1
   blocker, not a v1.0 blocker.

2. **Two independent implementations to verify normative behaviour.**
   The v1.0-RC2 spec is internally consistent but has not been
   re-implemented from scratch. A second implementation (even a
   partial one — lexer + parser + typeck) would surface ambiguities
   in the normative text. **This is the single largest v1.0 blocker.**
   Estimate: 2–4 weeks of dedicated work for an external
   re-implementer.

3. **Conformance corpus completeness audit.** The v0.8 consolidation
   noted that several §32 conformance category counts are `TBD`. v1.0
   freeze requires the corpus has at least one positive-fire test
   per FROZEN diagnostic code (currently 88 FROZEN codes, ~95% covered;
   `core_profile_rejects_alloc` notably awaits per-case `mighty.toml`
   overrides).

4. **F1 from v0.8: original `mighty_language_spec_v0_1.md`
   reconciliation.** If the project owner's external original spec
   becomes available, the v1.0-RC2 docs-pass should verify every
   normative claim. Not strictly blocking — v1.0-RC2 stands on its
   own as a normative document — but the cross-check would close any
   inadvertent divergence.

### Proposed v1.0 freeze date

Earliest plausible: **2026-09-01** (~14 weeks from v0.9).

Triggers (all must hold):

- Blocker #2 is satisfied: at least one external lexer + parser +
  typeck re-implementation has been built against v1.0-RC2 and the
  divergences have been reconciled.
- Blocker #3 is satisfied: conformance corpus completeness audit
  passes (positive-fire test per FROZEN code).
- All 6 RFCs have entered their comment windows (acceptance not
  required for v1.0 freeze; entry into the comment phase is enough
  to demonstrate the v1.1 path).
- The v0.10 / v0.11 / v0.12 slices land without surfacing a fresh
  contradiction in v1.0-RC2 that would require an RC3.

If a fresh contradiction surfaces, the freeze date slips by one
slice-cycle (~2–3 weeks).

---

## Post-v0.9 work flagged

In addition to the v1.0 freeze blockers above:

### G1 — RFC public comment infrastructure

Set up `github.com/hassard0/Mighty/discussions` (or the equivalent
on the post-rename repo) as the RFC comment forum. Each of the 6 RFCs
gets a discussion thread linked from the RFC doc's top-matter.

Owner: v0.10 slice.

### G2 — Design owners for each RFC

Each RFC is currently marked **Owner: *unassigned***. Before
v1.1-alpha, every RFC needs a named design owner who commits to
shepherding the RFC through the comment window and the implementation.

Owner: v0.10 slice (or community recruitment by v0.11).

### G3 — RFC-006 runtime primitives spike

The Detailed Design section of RFC-006 is flagged as draft-pending-
owner. A 1–2 week spike investigating tokio's per-task migration
options (custom executor, `tokio::task::JoinSet` re-binding) would
unblock the rest of the RFC. If the spike finds no viable approach,
RFC-006 slips to v1.2 and v1.1 ships only the `#[derive(Migratable)]`
compiler-side surface.

Owner: v0.10 or v0.11 slice; runtime expert required.

### G4 — Conformance corpus completeness sweep

See blocker #3 above. Tooling: extend the existing `cargo test --list`
classifier to map every emitted diagnostic code to its conformance
case, then list FROZEN codes without coverage. Target: zero gaps.

Owner: v0.11 or v0.12 slice.

### G5 — External re-implementation outreach

See blocker #2. Identify candidate re-implementers (the existing
language-design community, possibly a university course adopting
Mighty as a target language). Provide v1.0-RC2 as the spec, the
conformance corpus as the test suite. Estimate: 2–4 weeks of
dedicated external work.

Owner: project owner (relationship management).

### G6 — Rebrand the GitHub repo URL

The repo has been renamed to `hassard0/Mighty` per the v0.9
instructions, but the v1.0-RC2 spec text still references
`hassard0/stardust` in a few REBRAND_NOTES.md cross-links. Cosmetic
sweep needed before v1.0 freeze.

Owner: v0.10 docs slice.

---

## Verification

All work in this slice is docs-only. No crate sources were touched,
no `Cargo.toml` was touched, no test was modified. The pre-existing
`cargo test --workspace` corpus (per v0.8 release notes: 885+ tests
passing) is unchanged.

To verify the spec-freeze pass:

```bash
# All 10 OPEN amendments resolved
grep -c "^\*\*Status:\*\* OPEN" docs/spec/v0.1-amendments.md
# expect: 0 (all resolved to FREEZE-MVP or DEFER-V1.1)

# RFC count
ls docs/spec/rfcs/ | grep -c "^RFC-"
# expect: 6

# v1.0-RC2 title bump
grep "Release Candidate 2" docs/spec/v1.0-rc.md
# expect: at least one match (title + status line)

# CHANGELOG has v1.0-RC2 entry
grep -c "## v1.0-RC2" docs/spec/CHANGELOG.md
# expect: 1
```

---

## Report

**Status:** SHIPPED (spec-freeze prep slice).

- **Files added:** 6 RFCs under `docs/spec/rfcs/`,
  `SPEC_FREEZE_V0_9_NOTES.md`.
- **Files modified:** `docs/spec/v0.1-amendments.md` (10 status lines
  updated from OPEN to FREEZE-MVP or DEFER-V1.1),
  `docs/spec/v1.0-rc.md` (title bump + appendix updates),
  `docs/spec/CHANGELOG.md` (v1.0-RC2 entry).
- **OPEN amendments resolved:** 10 (3 FREEZE-MVP, 7 DEFER-V1.1, 0 DROP).
  FREEZE-MVP: A15, A31, A49. DEFER-V1.1: A11, A45, A47, A94, A97,
  A102, A103. (A100 remains FROZEN per v0.8 classification; its
  residual v1.1+ MtyIR-lower threading is tracked by RFC-004
  without changing A100's status.)
- **RFCs drafted:** 6 (RFC-001..RFC-006), all with at least Summary +
  Motivation + Detailed Design + Drawbacks + Alternatives Considered +
  Unresolved Questions + Adoption Plan; RFC-006's Detailed Design is
  flagged as draft-pending-owner.
- **v1.0 freeze blockers identified:** 4 (RFC comment windows,
  external re-implementation, conformance corpus completeness,
  optional reconciliation with original spec).
- **Proposed v1.0 freeze date:** 2026-09-01.
- **Tests touched:** 0 (docs-only).
- **Crate sources touched:** 0.
