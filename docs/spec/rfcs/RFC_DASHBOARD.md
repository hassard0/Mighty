# RFC Dashboard — v1.0 freeze gate

**Last updated:** 2026-05-26 (v0.24 Track D slice).
**Companion:** [`COMMENT_WINDOWS.md`](COMMENT_WINDOWS.md) — the
authoritative source for window dates and the policy text. This
file is the **live status view** that an integrator (or external
reviewer) opens to see, at a glance:

* Which windows are open, on what day they close, and how many days
  remain as of today.
* For each RFC: what's already been shipped in v0.13..v0.23 that
  reduces (or moots) the RFC's open question.
* Which RFCs are still pending **user action** — i.e. the
  GitHub Discussion thread has not been opened, or the close-out
  disposition has not been recorded.
* The single number a viewer cares about: **the day v1.0 can be
  tagged** (max close-date across all open windows, assuming no
  re-opens).

Today's date for the countdown is **2026-05-26**. All eight windows
opened on **2026-05-26** (see COMMENT_WINDOWS.md `## Changelog`),
so every "Days remaining" cell below is the window duration minus
zero. Refresh `Today` and the per-row deltas on each new slice.

## Top-line gauge

| Metric                                  | Value       |
|-----------------------------------------|-------------|
| Total RFCs in the v1.0 freeze gate      | **8**       |
| Windows currently **Open**              | **8**       |
| Windows **Closed** (accept / reject)    | **0**       |
| Windows pending **Modify and re-open**  | **0**       |
| Windows blocked on **Pending User Action** (Discussion thread not opened or close-out not recorded) | see notes below |
| Earliest close                          | **2026-06-09** (RFC-005) |
| Latest close (== earliest v1.0 tag day) | **2026-07-25** (RFC-002, RFC-006) |
| **Earliest possible v1.0.0 tag**        | **2026-07-26** |

**Pending User Action note.** Each row's "Pending User Action" flag
covers one or both of:

* **Thread open** — has the integrator created the
  `discussions/categories/rfcs` thread referenced by COMMENT_WINDOWS
  §1 and posted the RFC announcement? (Inferred today from whether
  the thread URL resolves. As of 2026-05-26, **all 8 threads are
  flagged Pending** — the v0.19 slice shipped the infrastructure
  but the user has not yet driven the Discussions side.)
* **Close-out recorded** — for windows that have already passed
  their close date, has the integrator written
  `dev/history/notes/RFC_DISPOSITION_<RFC>.md`? No window is
  past its close date as of 2026-05-26, so this column is N/A
  for every row.

## Per-RFC status (sorted by close date)

| RFC      | Title                              | Window  | Opened     | Closes     | Days left | Implementation status (v0.13..v0.23 work) | Feedback thread | Disposition       | Pending User Action                       |
|----------|------------------------------------|--------:|------------|------------|----------:|-------------------------------------------|-----------------|-------------------|-------------------------------------------|
| RFC-005  | Affinity frontend syntax           | 14 days | 2026-05-26 | 2026-06-09 |    **14** | A102 runtime API frozen v0.6; surface syntax NOT YET shipped (still reserved-but-not-parsed). RFC remains forward-looking. | [open](https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-005) | (pending)         | **Open thread**                           |
| RFC-001  | First-class union ADTs             | 30 days | 2026-05-26 | 2026-06-25 |    **30** | A11 sentinel lowering still in force in v1.0. RFC is forward-looking; nothing landed pre-empts it. | [open](https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-001) | (pending)         | **Open thread**                           |
| RFC-003  | Sandboxed proc-macro execution     | 30 days | 2026-05-26 | 2026-06-25 |    **30** | A94 parse + purity check still the v1.0 contract; MT6005/MT6006 active. No sandbox executor pre-built. RFC is forward-looking. | [open](https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-003) | (pending)         | **Open thread**                           |
| RFC-004  | Per-call FsCap manifest            | 30 days | 2026-05-26 | 2026-06-25 |    **30** | A100/A109 process-wide default still the v1.0 contract. RFC remains forward-looking. | [open](https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-004) | (pending)         | **Open thread**                           |
| RFC-008  | Effect rows                        | 30 days | 2026-05-26 | 2026-06-25 |    **30** | **SHIPPED v0.13..v0.19** — `HirEffectRow::Open(Vec<HirRowVar>)`, multi-row tail parsing, MT4055..MT4059 active. The window now closes the *process* (was there reviewer pushback?) rather than the design. | [open](https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-008) | (pending)         | **Open thread**; spec text already absorbed into v1.0-RC4 §9.2 |
| RFC-009  | Set-of-scopes macro hygiene        | 30 days | 2026-05-26 | 2026-06-25 |    **30** | **SHIPPED v0.13..v0.15** — `mty-macros` `scopes.rs`/`hygiene.rs`/`expand.rs`, scope-aware resolver wired into HIR. The RFC itself carries `Status: Accepted (v0.13)`. | [open](https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-009) | (pending — ratify) | **Open thread**; close-out is "ratify accepted" |
| RFC-002  | WASM Component Model wrapper       | 60 days | 2026-05-26 | 2026-07-25 |    **60** | A47/A97 core-module-only still the v1.0 contract. RFC remains forward-looking (canonical-ABI return bridge, wit-component wrap). | [open](https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-002) | (pending)         | **Open thread**                           |
| RFC-006  | Lossless live agent migration      | 60 days | 2026-05-26 | 2026-07-25 |    **60** | **SHIPPED v0.21 (Tier 4.3)** — `MigrationOrchestrator::migrate_agent`, `SnapshotSource`/`SnapshotSink`/mesh wire hooks, 6 MB cap, `PlacementPolicy` trait + 3 bundled policies, `[cluster.placement]` manifest block, OTel cluster metrics, MT507x band. RFC header already updated to "IMPLEMENTED in v0.21". | [open](https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-006) | (pending — ratify) | **Open thread**; close-out is "ratify shipped" |

> **Notes on already-shipped RFCs.** RFC-006, RFC-008, and RFC-009
> are coded against in production but their public comment window
> still has to run — the freeze policy in COMMENT_WINDOWS.md §3
> requires every RFC to *complete* a window before v1.0 tag, even
> if the design is already proven through implementation. The
> dispositions for these three will almost certainly be **accept**
> (ratifying the shipped behaviour), but reviewers may surface
> bugs / suggest refinements that we'd have to absorb as point
> patches in v1.0-PATCH1 or v1.1-MINOR1.

## Day-count visualisation

```
2026-05-26          2026-06-09          2026-06-25          2026-07-25
 |                   |                   |                   |
 Open (8 windows)    RFC-005 closes      RFC-001/003/004/    RFC-002/006 close
                     (14-day track)      008/009 close       (60-day track)
                                         (30-day track)
                                                              \
                                                               v1.0.0 earliest = 2026-07-26
```

## Per-RFC implementation cross-reference (deep dive)

This table feeds the per-RFC "Implementation Status" section that
each RFC file now carries. The dashboard cell above is the short
form; this is the verbose form.

| RFC      | Slices that touched it | Pre-window status |
|----------|------------------------|-------------------|
| RFC-001  | none in v0.13..v0.23   | Forward-looking. A11 sentinel `Result[T, Error]` remains v1.0 contract; first-class union ADTs are a v1.1 promotion. |
| RFC-002  | none in v0.13..v0.23   | Forward-looking. Core-Wasm-modules ship in v1.0 via `wasm-encoder`; `wit-component` wrap + canonical-ABI return bridge are v1.1 promotions. |
| RFC-003  | v0.13 stubbed sandbox  | Mostly forward-looking. MT6005 (impurity) + MT6006 (call-site gate) are the v1.0 contract; sandboxed execution is the v1.1 promotion. Some scaffolding for the eventual sandbox lives in `mty-macros::proc` constants but no executor. |
| RFC-004  | none in v0.13..v0.23   | Forward-looking. A100 process-wide-default `FsCap` remains the v1.0 contract; per-call manifest threading is the v1.1 promotion. |
| RFC-005  | none in v0.13..v0.23   | Forward-looking. `Affinity::Sticky`/`Elastic` runtime API frozen v0.6; surface syntax `agent X with affinity = sticky` is reserved-but-not-parsed. |
| RFC-006  | **v0.18..v0.21**       | **Implemented.** v0.18 cluster transport (`AgentAddr = node:type:pid` + framed CBOR-over-TLS); v0.19 routing + `MT5032` peer-disconnect fan-out; v0.20 mTLS + Tier 4.2 `ClusterSupervisor`; **v0.21 Tier 4.3 `MigrationOrchestrator` + snapshot wire variants + `PlacementPolicy` + manifest block + OTel cluster metrics + MT507x diagnostic band.** RFC header already carries `Status: IMPLEMENTED in v0.21`. |
| RFC-008  | **v0.13..v0.19**       | **Implemented.** v0.13 row variables landed in `mty-hir` / `mty-types`. v0.18 parser absorbed multi-row-variable tail (`!{| E1, E2}` / `effect a, b | E1, E2`); spec went RC3 → RC4 to admit the new grammar (see v1.0-rc.md §9.2). v0.19 finished HIR multi-row lowering (every `EFFECT_ROW_VAR` child read). MT4055..MT4059 all active emit. |
| RFC-009  | **v0.13..v0.15**       | **Implemented and Accepted.** `mty-macros::scopes` / `hygiene` / `expand`, scope-aware resolver into HIR, `expand_scoped` entry point. RFC file carries `Status: Accepted (v0.13)`. The comment window is procedural ratification. |

## How this dashboard stays accurate

* **Per-slice maintenance.** Every slice that touches RFC-adjacent
  code refreshes this file's per-RFC row and the in-file
  `Implementation Status` section in the matching RFC.
* **Per-day countdown.** "Days left" can be recomputed by hand
  from `(close - today)` in whole days; no script wires this yet
  (a `scripts/refresh-rfc-dashboard.py` is a v1.0-RC follow-up).
* **Disposition snapshot.** When a window closes, the
  integrator (a) writes
  `dev/history/notes/RFC_DISPOSITION_<RFC>.md`, (b) updates the
  `Status:` header line in the RFC file itself, and (c) flips this
  table's Disposition column.
* **Companion files.** This dashboard is **not** the source of truth
  for window opening dates — that's COMMENT_WINDOWS.md. Conflicts
  resolve in favour of COMMENT_WINDOWS.md (the policy doc).

## Changelog

* **2026-05-26** — initial dashboard authored as part of v0.24 Track D.
  All 8 windows recorded as open; 3 RFCs (006, 008, 009) flagged as
  already-implemented and awaiting procedural close-out.
