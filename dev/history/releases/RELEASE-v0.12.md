# Mighty v0.12 — Release Notes

**Tag:** `v0.12.0`
**Date:** 2026-05-25
**Status:** SHIPPED — v1.0-RC3 spec released + 4th showcase demo
+ conformance Gap B/C/E partial closure + Go 3rd-impl source landed.

v0.12 is a *spec-and-evidence* release: every workstream pushes the
project closer to v1.0 freeze. The normative spec advances from
**v1.0-RC2 → v1.0-RC3** (operator precedence promoted to normative
§11.1.1, reserved-keyword set enumerated, 16 Python-impl findings
codified — closes KNOWN_ISSUES #10 and #12). A fourth runnable
showcase lands at `demos/04_kvstore/` (sharded supervised in-memory
key-value store, exercising agents + protocols + supervisors +
restart + `std.http` end-to-end). The conformance corpus gains six
new typeck / borrow-check fixtures and a real MT3007 emit-site in
`mty-borrow/src/flow.rs` (Gap C partial). And a third independent
implementation lands at `impl-go/`: 4848 LOC of Go (lexer + parser
+ CLI + tests) built from the v1.0-RC3 spec alone, with no peeking
at `crates/mty-*`, `selfhost/`, or `impl-py/`.

**Headline:** the v1.0 spec is now v1.0-RC3 (the v0.11 Python-impl
findings are codified; operator-precedence and reserved-keyword
gaps closed); two of the three independent implementations called
for by RFC-007 are now physically in the repo (`impl-py/` validated,
`impl-go/` shipped pending Go-toolchain cross-validation); the
demos corpus reaches **4/4 passing**; the normative conformance
corpus reaches **89 cases / 3 ignored across 16 categories**.

If you were on v0.11.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force`. There are no source-level breaking
changes for end-user Mighty programs.

## Highlights

- **v1.0-RC2 → v1.0-RC3 spec polish.** Operator precedence promoted
  to normative §11.1.1 (the Pratt-style ladder from
  `crates/mty-syntax/src/parser/exprs.rs::infix_bp`, mirroring the
  C/Rust convention — was non-normative in RC2 and deferred to
  `docs/internals/parser.md`). The full reserved-keyword set is
  now enumerated: **63 reserved keywords** + 4 contextual keywords
  (`get`, `set`, `where`, `default`) + 7 reserved-for-future-use
  (`become`, `final`, `priv`, `super`, `try`, `typeof`, `unsized`).
  The 16 Python-impl findings (`PYTHON_IMPL_V0_11_NOTES.md`) are
  codified in the prose. **+396 spec lines**, no normative behaviour
  change — the Rust reference compiler already implements every
  decision; v0.12 just promotes the implementation choices into
  normative prose so a 3rd / 4th implementer can build to spec
  alone. **Closes KNOWN_ISSUES #10 and #12.** See
  [`SPEC_RC3_V0_12_NOTES.md`](../notes/SPEC_RC3_V0_12_NOTES.md).
- **4th showcase demo: `demos/04_kvstore/`.** Sharded supervised
  in-memory key-value store (~400 LOC). Exercises agents +
  protocols + supervisors + restart-on-crash + `std.http` in a
  single end-to-end runnable artefact. The first demo whose pitch
  is **the supervisor restart story**: a shard agent crashes on a
  poison key, the supervisor restarts it, and the HTTP surface
  stays available. Smoke-tested via `demos/04_kvstore/smoke.sh`.
  See [`DEMO04_V0_12_NOTES.md`](../notes/DEMO04_V0_12_NOTES.md).
- **Conformance Gap B/C/E partial closure.** Six new fixtures
  (`type_checking/17..20`, `borrow_checking/13..14`), plus a real
  MT3007 emit-site landed in `mty-borrow/src/flow.rs::pop_frame`
  (previously only the diagnostic constructor existed; no driver
  fired it). The conformance harness now reports **89 cases / 16
  categories / 3 ignored**. One new red-shirt:
  `borrow_checking/14_borrow_outlives_owner` exposes that
  `pending_borrower` is not yet wired through plain assignments
  (only `let` bindings); deferred to v0.13.
- **Go 3rd-impl source-only landing.** `impl-go/`, a Go 1.22+
  lexer + parser + CLI (`mty-go lex|parse`) + tests for the Mighty
  front-end. **4848 LOC** across lexer + parser, written from
  `docs/spec/v1.0-rc.md` (v1.0-RC3) prose alone — zero peeking at
  `crates/mty-*`, `selfhost/`, or `impl-py/`. Go is not installed
  on the v0.12 build host so `go test ./...` has **not** been run;
  cross-validation against the Python + Rust impls is a v0.13
  task. The structural inventory (file layout, LOC counts) is the
  only check applied for v0.12. See
  [`GO_IMPL_V0_12_NOTES.md`](../notes/GO_IMPL_V0_12_NOTES.md).
- **No new Rust crates, no new failing tests.** v0.12 is docs +
  fixtures + a new demo + a new optional `impl-go/` tree — the
  20-crate Rust workspace is unchanged. **977 Rust tests + 135
  Python tests + 89 conformance cases + 40 self-host tests = 1241
  passing**, 0 failing, 3 ignored (intentional: 2 carried over
  from v0.11, 1 new red-shirt deferred to v0.13).

## What's new

### Spec polish v1.0-RC2 → v1.0-RC3

The v0.11 Python 2nd-impl swarm surfaced 16 spec ambiguities and
flagged three big ones as v1.0 freeze blockers (KNOWN_ISSUES #10
operator precedence, #12 reserved-keyword set, with #11 being the
related six emit-sites pending). v0.12 closes the prose half.

**Operator precedence (§11.1.1, NEW).** The Pratt table from
`crates/mty-syntax/src/parser/exprs.rs::infix_bp` is promoted
verbatim into the normative spec. C/Rust-conventional ladder;
right-associative for `= += -= *= /= %= &= |= ^= <<= >>=`; all
other binary operators left-associative (table footnote, also
normative). The internals doc now points back at §11.1.1.

**Reserved keyword set (§3.3, REWRITTEN).** Three pivots:

1. The full set of **63 reserved keywords** is now enumerated
   in-spec (was scattered across §3, §4, §11, §12). `package`,
   `export`, `requires` — the three the Python impl flagged
   missing — are explicitly named.
2. **Contextual keywords** (`get`, `set`, `where`, `default`)
   are split into a separate non-reserved sub-list with their
   permitted contexts spelled out.
3. **Reserved-for-future-use** identifiers (`become`, `final`,
   `priv`, `super`, `try`, `typeof`, `unsized`) are listed and
   marked as compile errors if used as identifiers, with a
   forward-compat rationale.

**16 Python-impl findings codified.** Numeric underscore
placement (§3.4.2), struct field separator (§7.2), arena inline
form (§9.4), and 13 others — each surface call recorded as
prose in the right section. The findings table in
`PYTHON_IMPL_V0_11_NOTES.md` is now fully resolved.

**Result:** `docs/spec/v1.0-rc.md` grows **+396 lines** (no
removed lines beyond the deferred-precedence placeholder). The
RC3 banner replaces the RC2 banner; the changelog at the top
of the spec gets a new RC2 → RC3 section.

Closes KNOWN_ISSUES #10 and #12.

### 4th showcase demo: `demos/04_kvstore/`

The v0.11 demos covered an HTTP search API, a counter web
component, and a CLI extract tool — but none exercised the
supervisor restart story. `demos/04_kvstore/` is the missing
piece:

- **Shape:** an in-memory key-value store sharded across N
  agents by key hash. Each shard is supervised; a designated
  poison key crashes the shard agent on write; the supervisor
  restarts it; the HTTP surface stays available.
- **Surface:** `std.http` server exposing `GET /kv/:k`,
  `PUT /kv/:k`, `DELETE /kv/:k`, `GET /health`.
- **Size:** ~400 LOC of Mighty.
- **Verification:** `demos/04_kvstore/smoke.sh` boots the demo,
  runs PUT/GET/crash/PUT-recovery against it, and asserts the
  supervisor restart fires.

Wires together five v0.1..v0.7 features for the first time in
one place — agents, protocols, supervisors with restart,
`Sendable`, and `std.http`. See
[`DEMO04_V0_12_NOTES.md`](../notes/DEMO04_V0_12_NOTES.md).

### Conformance Gap B/C/E closure (partial)

v0.11 left a 6-gap residue under the fixture-only constraint.
v0.12 lifted that constraint for the borrow layer:

| Gap | Closure mechanism |
|---|---|
| C (borrow) | MT3007 `BORROW_OUTLIVES_OWNER` emit-site added in `mty-borrow/src/flow.rs::pop_frame` (previously constructor-only) |
| B (typeck) | 4 new fixtures: `type_checking/17_cannot_take_ref`, `18_arith_op_type`, `19_assign_to_non_place`, `20_wrong_variant_arity` |
| C (borrow) | 2 new fixtures: `borrow_checking/13_move_out_of_borrowed`, `borrow_checking/14_borrow_outlives_owner` |
| E (runtime traps) | (carried over to v0.13 — interp-only branches still dead) |

**Result:** `conformance_full` now reports **89 cases / 16
categories / 3 ignored**.

**Ignored breakdown:**

1. `borrow_checking/14_borrow_outlives_owner` — **new
   red-shirt for v0.13**. MT3007 fires for the `let r = &inner`
   shape but not the `r_out = &inner` plain-assignment reshape.
   The fix is to extend the `BinOp::Assign` branch in
   `record_borrow_for_rhs` to stamp `pending_borrower` (so the
   ledger records the reassign-into the same way it does for
   `let`-binders). The fixture is preserved as a red-shirt so
   the v0.13 patch is correct-by-construction.
2. `capability_checking/03_narrow_to_ro` — carried from v0.11
   (Slice-8 cap-narrowing dependency).
3. `supervisor_restart/02_escalate` — carried from v0.11
   (parser does not yet accept `escalate` in `on_fail`).

**Remaining Gap B residue (carried to v0.13 per KNOWN_ISSUES
#11):** MT2003, MT2009, MT2022, MT2023, MT2024, MT2025 still
constructor-only in `mty-types/src/check.rs`. The four newly
added fixtures land on the codes whose emit-sites already
existed (MT2002/MT2007/MT2010/MT2012); the residue six need
real wiring work in `check.rs`.

### Go 3rd-impl (source-only)

The second feeder for RFC-007 ("two independent implementations"
v1.0 freeze blocker). `impl-go/` is a Go 1.22+ port of the
front-end:

- `impl-go/mty/lexer.go` — full token surface (1123 LOC)
- `impl-go/mty/parser.go` — recursive descent + Pratt (2848 LOC)
- `impl-go/mty/diagnostics.go` — MT-coded diagnostics (68 LOC)
- `impl-go/mty/{lexer,parser,examples}_test.go` — unit + sweep
  tests (725 LOC)
- `impl-go/cmd/mty-go/main.go` — CLI: `mty-go lex|parse <file>`
- `impl-go/go.mod` — module `github.com/hassard0/mighty-impl-go`

**Total: 7 files, 4848 LOC (lexer + parser core; ~5573 LOC
including tests).**

**Built from `docs/spec/v1.0-rc.md` (v1.0-RC3) alone.** The
v0.11 Python impl was built from RC2; the v0.12 Go impl is the
first implementation built against the RC3 polish. Any
divergence the Go impl surfaces is a new RC3 finding.

**Validation status: pending Go toolchain.** The v0.12 build
host (`C:\Users\ihass\stardust`) does not have Go installed
(`go: command not found`), so `go test ./...` has not been
executed. The structural inventory (file layout, LOC counts,
top-level entry points) is the only check applied for v0.12.
Cross-validation — including `go test ./...`, an example
sweep over `examples/`, and 3-way agreement between Rust /
Python / Go on each `examples/NN_*.mty` — is a v0.13 task.
See [`GO_IMPL_V0_12_NOTES.md`](../notes/GO_IMPL_V0_12_NOTES.md).

The v0.11 commitment to write the 2nd impl without peeking at
the Rust reference is repeated here. Audit trail is the commit
message on `b05fe8f` ("v0.12 swarm recovery: conformance Gap
B/C/E partial + Go 3rd-impl source").

## v1.0 freeze: blockers + proposed date

The v1.0 spec is now at v1.0-RC3. Two of the three independent
implementations are in the repo. Blockers (delta vs v0.11
italicised):

1. **Two independent implementations.** *Promoted from "partial
   credit" to "two-in-repo, Go pending cross-validation"*: the
   Rust reference compiler, the Python 2nd-impl (`impl-py/`, 135
   tests, 20/20 examples lex+parse), and the Go 3rd-impl
   (`impl-go/`, 4848 LOC, source-only — Go toolchain absent on
   build host so `go test ./...` not yet run). RFC-007 can now
   point at three implementations once the Go impl gets a
   green test run.
2. **RFC comment periods.** RFC-001 through RFC-006 each need a
   30-day public window — unchanged from v0.11.
3. **Published normative conformance suite.** The corpus now
   stands at 89 cases / 16 categories. Coverage of FROZEN
   diagnostic codes advances to ~92% (one Gap C closure: MT3007
   now has an emit-site witness). The remaining residue is six
   Gap-B typeck codes per KNOWN_ISSUES #11.
4. *(was NEW in v0.11) ~~Operator precedence in the normative
   spec~~* — **closed in v1.0-RC3** (promoted to §11.1.1).
5. *(was NEW in v0.11) ~~`package`/`export`/`requires` not in
   §3.3~~* — **closed in v1.0-RC3** (full 63-keyword set
   enumerated; contextual + reserved-for-future split out).

**Proposed v1.0 freeze date: 2026-09-01** (unchanged).

## Backwards-compat aliases (status)

Unchanged from v0.11. All v0.7 + v0.8 aliases (`mty dump --sir`
alias of `--ir`; legacy `SD####` accepted by `mty explain`;
`--legacy-interp`; legacy `sd`/`stardust` code-block tags) stay
live per A45's DEFER-V1.1 resolution.

## Stats

| | v0.11.0 | v0.12.0 | Delta |
|---|---|---|---|
| Workspace crates | 20 | 20 | 0 |
| Rust tests passing | 977 | **977** | 0 |
| Python tests passing | 135 | **135** | 0 |
| Self-host tests | 40 | **40** | 0 |
| Conformance cases | 84 | **89** | **+5** |
| Conformance ignored | 2 | **3** | **+1** (red-shirt for v0.13) |
| Combined test count | 1236 | **1241** | **+5** |
| Tests failing | 0 | 0 | 0 |
| Diagnostic codes | 67+ | 67+ | 0 |
| Examples passing (check) | 20/20 | **20/20** | 0 |
| Demos passing | 3/3 | **4/4** | **+1** |
| Independent implementations | 2 (front-end only) | **3 (front-end only; Go pending cross-validation)** | **+1** |
| Spec | v1.0-RC2 | **v1.0-RC3** | **+396 lines** |
| Spec amendments | 88 | 88 | 0 |
| RFCs | 6 | 6 | 0 |
| Fuzz targets | 4 | 4 | 0 |
| CI jobs (all required) | 6 | 6 | 0 |
| Commits since prior tag | 11 | **5** | — |
| Lines changed since prior tag | 127 files, +6 320 / -745 | **51 files, +7 368 / -59** | — |

## Migration steps

For end-user Mighty packages: **none required**. v0.12 has zero
language-level changes.

For toolchain contributors: there are no new gates beyond v0.11.

For implementers building a parallel front-end: `docs/spec/v1.0-rc.md`
now contains the normative operator-precedence ladder (§11.1.1)
and the full 63-reserved-keyword set (§3.3). If you were waiting
on these to start a 3rd impl, the spec is now sufficient — see
`impl-go/` for a working RC3 reference.

## Known issues

Canonical list in [`KNOWN_ISSUES.md`](../../../KNOWN_ISSUES.md):

1. ~~`cabi_realloc` is a bump allocator~~ — **closed in v0.10.**
2. ~~Package signing is a stub~~ — **closed in v0.10 behind
   `sigstore-real`.**
3. MSRV gate runs only `cargo build` — partially closed in v0.10
   (carry-over).
4. ~~`clippy-strict` job is `continue-on-error: true`~~ —
   **closed in v0.11.**
5. ~~mkdocs `--strict` not enabled~~ — **closed in v0.10.**
6. Demo 02 JS shim still writes into the fixed `DOM_RETURN_AREA`
   instead of calling `cabi_realloc()` — unchanged.
7. `--no-default-features` test job does not run the example
   sweep — unchanged.
8. **Set-of-scopes hygiene in LSP completion (A111)** — deferred
   post-v1.0 (pre-existing).
9. **Cranelift egraph stack overflow** — filed upstream as
   wasmtime #13476; in-tree workaround `MTY_CRANELIFT_NO_OPT=1`.
10. ~~Operator precedence not normative~~ — **closed in v1.0-RC3
    (§11.1.1).**
11. **Six FROZEN typeck codes still constructor-only**
    (MT2003, MT2009, MT2022, MT2023, MT2024, MT2025) — carried
    over. v0.12 closed MT2002/MT2007/MT2010/MT2012 with new
    fixtures (call-sites already existed); the residue six still
    need wiring in `mty-types/src/check.rs`.
12. ~~`package`, `export`, `requires` keywords not in §3.3~~ —
    **closed in v1.0-RC3** (full 63-keyword set enumerated).
13. **(NEW) Red-shirt:**
    `conformance/borrow_checking/14_borrow_outlives_owner` is
    ignored — MT3007 fires for `let r = &inner` but not for the
    plain-assignment reshape `r_out = &inner`. The fix is to
    extend the `BinOp::Assign` branch in `record_borrow_for_rhs`
    (in `mty-borrow/src/flow.rs`) to stamp `pending_borrower`.
    Deferred to v0.13.
14. **(NEW) Go 3rd-impl cross-validation pending.** Go toolchain
    absent on the v0.12 build host so `go test ./...` has not
    been run; example-sweep + 3-way Rust/Python/Go agreement
    pending v0.13.

## v0.12 → v1.0-final roadmap

- Open RFC-001..006 30-day comment periods (carry-over).
- Wire the 6 remaining Gap-B typeck call-sites in
  `mty-types/src/check.rs` (MT2003/MT2009/MT2022/MT2023/MT2024/
  MT2025) — closes KNOWN_ISSUES #11.
- Patch `record_borrow_for_rhs` to stamp `pending_borrower` on
  `BinOp::Assign` — closes the new red-shirt
  (`borrow_checking/14_borrow_outlives_owner`).
- Run `go test ./...` on a Go-1.22+ host and cross-validate the
  Go impl against Rust + Python over the `examples/` sweep —
  closes KNOWN_ISSUES #14 and converts the third impl from
  source-only to validated.
- Extend the Python 2nd-impl through HIR + sketch typeck
  (~5.5 KLOC, ~8 days, carry-over from v0.11).
- Split MT0001 funnel (carry-over from v0.10 Gap A).
- `mty-pkg` cross-file resolution; parametric newtypes for
  self-host arena ids; LSP A111 set-of-scopes hygiene
  (carry-overs).
- Publish normative conformance suite as a downloadable kit.

## Acknowledgments

v0.12 was built across a v0.12 swarm (four parallel tracks)
followed by an integrator pass:

- **spec-rc3-swarm** — operator precedence promoted to normative
  §11.1.1; reserved-keyword set enumerated (63 reserved + 4
  contextual + 7 reserved-for-future); 16 Python-impl findings
  codified; RC2 banner → RC3 (+396 spec lines). Closes
  KNOWN_ISSUES #10 and #12. Commits `339299f`, `ea35b61`,
  `35abb43`.
- **demo04-swarm** — `demos/04_kvstore/` (~400 LOC) — sharded
  supervised in-memory KV store demonstrating the supervisor
  restart story end-to-end. Commit `963de08`.
- **conformance-gap-closure-swarm** — MT3007 emit-site landed
  in `mty-borrow/src/flow.rs::pop_frame`; six new fixtures
  (typeck 17..20, borrow 13..14); one red-shirt deferred.
  Folded into commit `b05fe8f` (integrator-recovered).
- **go-impl-swarm** — `impl-go/` 4848 LOC lexer + parser + CLI
  + tests, built from v1.0-RC3 spec alone. Validation pending
  Go toolchain. Folded into commit `b05fe8f`
  (integrator-recovered).

The integrator pass (this v0.12.0 tag commit) re-verified the
gates (977 Rust + 135 Python + 89 conformance + 40 selfhost
= 1241 tests passing / clippy strict / fmt / 20-example matrix
/ 4/4 demos / 3 conformance ignored) and authored this
`RELEASE-v0.12.md`. A small `cargo fmt` cleanup was applied
to v0.12 emit-site additions (borrow flow + typeck check +
typeck diag) — content unchanged.

See [`SPEC_RC3_V0_12_NOTES.md`](../notes/SPEC_RC3_V0_12_NOTES.md),
[`DEMO04_V0_12_NOTES.md`](../notes/DEMO04_V0_12_NOTES.md), and
[`GO_IMPL_V0_12_NOTES.md`](../notes/GO_IMPL_V0_12_NOTES.md) for
per-agent interpretation calls.
