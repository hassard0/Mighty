# v0.24 — v1.0 freeze-preparation notes (Track D)

This is the v0.24 Track D session log. v0.24 is the post-RC slice
whose only job is to put the spec, RFC dashboard, and conformance
kit into v1.0 GA-ready shape. All eight RFC comment windows opened
2026-05-26; the earliest possible v1.0 tag is 2026-07-26.

## What this slice shipped (4 file groups)

### Group 1 — RFC monitoring dashboard

* **NEW:** [`docs/spec/rfcs/RFC_DASHBOARD.md`](../../../docs/spec/rfcs/RFC_DASHBOARD.md) —
  the live status view that an integrator (or external reviewer)
  opens to see, at a glance: which windows are open, how many days
  remain, which RFCs are already implemented in v0.13..v0.23, and
  which RFCs are pending user action (Discussion thread not opened
  yet).
* **EDIT:** [`docs/spec/rfcs/COMMENT_WINDOWS.md`](../../../docs/spec/rfcs/COMMENT_WINDOWS.md) —
  added cross-reference to the dashboard at the top + changelog
  entry documenting the v0.24 dashboard addition. No window dates
  changed.
* **EDIT:** Each of the 8 RFC files
  (`RFC-001` .. `RFC-006`, `RFC-008`, `RFC-009`) gained an
  `## Implementation Status` section noting what's already shipped
  vs what remains forward-looking. Five are forward-looking (001,
  002, 003, 004, 005); three are shipped pending procedural
  ratification (006, 008, 009).

### Group 2 — RC4 → RC5 spec polish

`docs/spec/v1.0-rc.md` walked from RC4 to RC5. All changes are
additive (no behaviour change for v1.0-conforming programs):

* **§12.6** — `Resumable` trait + `reload::swap` pause/drain/
  snapshot/schema-check/restore/resume pipeline + `ReloadGate`
  drain + `MigrateFrom<Old>` + `SchemaRegistry` BFS for
  schema-evolution chains + `__mty_agent_type` / `__mty_schema_hash`
  wasm custom sections. The v0.20 + v0.21 Tier 1.5 work promoted
  to normative.
* **§12.7** — `MT506x` reload-band diagnostic table.
* **§12.8** — `mty reload <agent-type> --from new.wasm` and
  `mty serve [--port <n>] [--watch]` control surfaces.
* **§22.5** — `mty:web/canvas@0.1` + `mty:web/input@0.1` WIT
  interfaces + `std.web.{Canvas, Input}` Mighty-side bindings
  promoted to normative. Drift-guard via `WIT_IMPORT_*` /
  `WIT_EXPORT_*` consts.
* **§25.8** — Cluster mesh + cluster supervisor + lossless live
  migration + `PlacementPolicy` + manifest block + telemetry +
  `MT503x` / `MT507x` diagnostic bands promoted to normative.
  Splits into 8 numbered subsections (§25.8.1 .. §25.8.8). The
  v0.18 transport + v0.19 routing + v0.20 mTLS + Tier 4.2 +
  v0.21 Tier 4.3 work.
* **§27.1** — `std.web` module row added to the stdlib table.
* **§20.6** — `format!` builtin macro promoted to normative.
  Conversion-spec table (`{}`, `{:x}`, `{:X}`, `{:b}`, `{:o}`,
  `{:?}`), named placeholders, MT6009 / MT6010 error codes. Track
  B shipped the expander + `std.fmt` contract; the spec text
  documents the v1.0 contract. Bare-`{expr}` interpolation
  literals remain DEFER-V1.1.
* **§32** — conformance suite table refreshed to the 24-category
  / 153-case shipping shape; cross-links to the new normative
  classification doc.
* **§A.1** — FROZEN matrix extended with Cluster mesh, Hot reload,
  and `std.web` headings.
* **§A.2** — `format!()` recorded as a DEFER-V1.1 line. RFC-006
  row footnoted as already-shipped (the row remains until the
  RFC-006 comment window closes).
* **§C.1** — RFCs table extended from 6 → 8 rows (added RFC-008,
  RFC-009) + an Implementation Status column added.
* **Version label** — RC4 → RC5 in the title + front-matter +
  closing "End of …" line.

The polish absorbs about **+330 lines** of normative prose into
the spec. No existing text was removed.

### Group 3 — Conformance kit v1.0-GA readiness

* **NEW:** [`tests/conformance/v1.0-NORMATIVE.md`](../../../tests/conformance/v1.0-NORMATIVE.md) —
  the v1.0 GA normative-vs-informative declaration. 153 cases
  classified as **104 normative / 49 informative** across 24
  categories.
* **EDIT:** [`tests/conformance/CONFORMANCE_KIT.md`](../../../tests/conformance/CONFORMANCE_KIT.md) —
  v1.0 GA readiness banner at the top + manifest table refreshed
  to 153 cases with a v1.0 GA bucket column.

The 5 informative categories — `runtime/`, `runtime-7/`,
`codegen/`, `native_abi/`, `wasm_component/` — are all backend-
specific. A front-end-only impl can claim v1.0 conformance by
passing 104/104 normative cases and documenting the 49 skipped
informative cases.

### Group 4 — README and KNOWN_ISSUES

* **EDIT:** [`README.md`](../../../README.md) — spec badge v1.0-RC4
  → v1.0-RC5; `## Status` paragraph; `## Roadmap → To v1.0` block
  refreshed with dashboard pointers and the v0.24 normative-split
  call-out.
* **VERIFY:** [`KNOWN_ISSUES.md`](../../../KNOWN_ISSUES.md) —
  re-verified. P0 has no entries (the brief said "verify");
  every P1/P2 carries a "RESOLVED" or "RESOLVED v0.1x" status
  note (resolutions span v0.10 → v0.19). The file remains the
  historical record; no new entries needed for v0.24.

## Pending User Action

The eight RFC GitHub Discussion threads (one per RFC under
`hassard0/Mighty/discussions/categories/rfcs`) have not been opened
by the user. The dashboard's `Pending User Action` column flags
all 8 rows. The window opening dates and policy in
COMMENT_WINDOWS.md are valid regardless; the Discussion threads
are the **feedback channel**, not the timer.

To unblock v1.0:

1. Create the `rfcs` Discussion category (one-time).
2. Open 8 threads with the RFC text + the close-date.
3. As windows close (RFC-005 first on 2026-06-09), write
   `dev/history/notes/RFC_DISPOSITION_<RFC>.md` per the closing
   protocol in COMMENT_WINDOWS.md §3.

## Pre-flight verification

* `mkdocs build --strict` — passes, no errors. The pre-existing
  INFO-level link notes (about non-spec docs referencing
  `v0.1-amendments.md` anchors that don't exist) are unchanged.
* `cargo build --workspace` — passes (8.34s; no crate touched
  this slice).
* `cargo fmt --all -- --check` — passes (no Rust changed).

## Deltas by file

| File                                                      | Delta            |
|-----------------------------------------------------------|------------------|
| `docs/spec/rfcs/RFC_DASHBOARD.md`                         | new (~110 lines) |
| `docs/spec/rfcs/COMMENT_WINDOWS.md`                       | +~15 lines       |
| `docs/spec/rfcs/RFC-001-first-class-union-adts.md`        | +~20 lines       |
| `docs/spec/rfcs/RFC-002-wasm-component-model-wrapper.md`  | +~30 lines       |
| `docs/spec/rfcs/RFC-003-sandboxed-proc-macro-execution.md`| +~20 lines       |
| `docs/spec/rfcs/RFC-004-per-call-fscap-manifest.md`       | +~25 lines       |
| `docs/spec/rfcs/RFC-005-affinity-frontend-syntax.md`      | +~30 lines       |
| `docs/spec/rfcs/RFC-006-lossless-live-agent-migration.md` | +~35 lines       |
| `docs/spec/rfcs/RFC-008-effect-rows.md`                   | +~25 lines       |
| `docs/spec/rfcs/RFC-009-set-of-scopes.md`                 | +~20 lines       |
| `docs/spec/v1.0-rc.md`                                    | +~330 lines      |
| `tests/conformance/v1.0-NORMATIVE.md`                     | new (~160 lines) |
| `tests/conformance/CONFORMANCE_KIT.md`                    | +~15 lines edits |
| `README.md`                                               | +~10 lines edits |
| `dev/history/notes/V1_FREEZE_PREP_V0_24_NOTES.md`         | new (this file)  |

## What's left for v1.0 GA

Only the eight RFC comment windows. Earliest possible tag day is
**2026-07-26** (the day after RFC-002 and RFC-006 close, assuming
no re-opens).
