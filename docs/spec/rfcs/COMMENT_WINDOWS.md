# RFC Comment Windows

This document tracks the public comment windows for the Mighty
language-spec RFCs. The v1.0 freeze process requires each RFC to have
a public comment window open for the duration listed in its header
before it can be normative-accepted into `docs/spec/v1.0-rc.md`. This
document is the single source of truth for **which window is open,
when each closes, and how feedback is received**.

The v0.19 slice ships this tracking infrastructure. The actual
opening of each window (creating the GitHub Discussion thread,
sending the announcement email, posting to the mailing list) is a
user-driven admin action — this table records the dates the user
opens each window so reviewers can plan their feedback.

## Window status

| RFC      | Title                                | Comment window | Opened       | Closes       | Status   |
|----------|--------------------------------------|---------------:|--------------|--------------|----------|
| RFC-001  | First-class union ADTs               | 30 days        | 2026-05-26   | 2026-06-25   | **Open** |
| RFC-002  | WASM Component Model wrapper         | 60 days        | 2026-05-26   | 2026-07-25   | **Open** |
| RFC-003  | Sandboxed proc-macro execution       | 30 days        | 2026-05-26   | 2026-06-25   | **Open** |
| RFC-004  | Per-call fscap manifest              | 30 days        | 2026-05-26   | 2026-06-25   | **Open** |
| RFC-005  | Affinity frontend syntax             | 14 days        | 2026-05-26   | 2026-06-09   | **Open** |
| RFC-006  | Lossless live agent migration        | 60 days        | 2026-05-26   | 2026-07-25   | **Open** |
| RFC-008  | Effect rows                          | 30 days        | 2026-05-26   | 2026-06-25   | **Open** |
| RFC-009  | Set-of-scopes                        | 30 days        | 2026-05-26   | 2026-06-25   | **Open** |

Earliest close: **2026-06-09** (RFC-005).
Latest close:    **2026-07-25** (RFC-002, RFC-006).
Overall status:  **Open** through 2026-07-25.

A window may be re-opened (with a new opening date and a fresh
duration) if material changes are made to the RFC during the window —
this is at the integrator's discretion and SHOULD be rare.

## Window duration policy

Each RFC's window duration is keyed to its surface area:

* **14 days** — syntactic / cosmetic refinements with a small
  user-visible footprint (RFC-005 affinity syntax).
* **30 days** — typical RFCs that add or refine a single language
  feature without changing core semantics (RFC-001, RFC-003, RFC-004,
  RFC-008, RFC-009).
* **60 days** — cross-cutting RFCs that touch the runtime model or
  binary-format contract (RFC-002 WASM CM wrapper, RFC-006 live
  agent migration).

If the spec adds new RFCs after v1.0, the duration table above is the
default. Authors MAY request a longer window in the RFC header; the
integrator MAY shorten only with a documented reason.

## How feedback is received

Three channels, in order of preference:

### 1. GitHub Discussions (primary)

A category per RFC under
<https://github.com/hassard0/Mighty/discussions/categories/rfcs>.
Create the category if it does not exist (an admin one-time step —
this document does not auto-create it). Each RFC's thread is the
canonical venue for technical discussion; substantive comments
SHOULD land there so the conversation is publicly archived.

Discussion link format (one per RFC):

* RFC-001: <https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-001>
* RFC-002: <https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-002>
* RFC-003: <https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-003>
* RFC-004: <https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-004>
* RFC-005: <https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-005>
* RFC-006: <https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-006>
* RFC-008: <https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-008>
* RFC-009: <https://github.com/hassard0/Mighty/discussions/categories/rfcs?discussions_q=RFC-009>

### 2. Inbound-notes files (asynchronous)

For feedback that arrives via email / chat / phone-call / in-person
that doesn't make it to Discussions, the integrator transcribes a
summary into `dev/history/notes/RFC_FEEDBACK_<RFC>.md`. These files
are also where multi-party email threads can be summarised after the
fact.

Convention: one file per RFC, append-only, with timestamps. A new
section per inbound piece of feedback. The integrator is the only
writer.

### 3. PR comments on the RFC document itself (last resort)

Some reviewers prefer commenting inline on the RFC markdown via a PR.
These comments are valid feedback but SHOULD be cross-posted to the
Discussions thread so the conversation is searchable in one place.

## Closing protocol

When an RFC's window closes:

1. The integrator collects all feedback (Discussions, notes file, PR
   comments).
2. The integrator decides one of: **accept**, **reject**, or **modify
   and re-open**. A rationale is recorded in
   `dev/history/notes/RFC_DISPOSITION_<RFC>.md`.
3. If **accept**: the RFC is moved into `docs/spec/v1.0-rc.md` normative
   text, the RFC file gets a header line `Status: accepted YYYY-MM-DD`,
   and this table's Status column flips to "Accepted".
4. If **reject**: the RFC file gets a header line
   `Status: rejected YYYY-MM-DD`, and this table flips to "Rejected".
5. If **modify and re-open**: the RFC is edited, a new opening date
   is recorded above (NEW row appended to the per-RFC history below),
   and the window restarts.

## Per-RFC opening history

(Append-only. The current "Opened / Closes" entry in the table above
is always the most recent.)

| RFC     | Iteration | Opened       | Closes       | Closing outcome |
|---------|-----------|--------------|--------------|------------------|
| RFC-001 | 1         | 2026-05-26   | 2026-06-25   | (pending)        |
| RFC-002 | 1         | 2026-05-26   | 2026-07-25   | (pending)        |
| RFC-003 | 1         | 2026-05-26   | 2026-06-25   | (pending)        |
| RFC-004 | 1         | 2026-05-26   | 2026-06-25   | (pending)        |
| RFC-005 | 1         | 2026-05-26   | 2026-06-09   | (pending)        |
| RFC-006 | 1         | 2026-05-26   | 2026-07-25   | (pending)        |
| RFC-008 | 1         | 2026-05-26   | 2026-06-25   | (pending)        |
| RFC-009 | 1         | 2026-05-26   | 2026-07-25   | (pending)        |

Note: RFC-009 window above pegs to the 60-day option; the active row
in the status table is the source of truth. Update both rows when an
RFC re-opens.

## Relationship to v1.0 freeze

v1.0 cannot be tagged until every RFC has its first window closed
with an accept-or-reject disposition (modify-and-re-open is allowed
before v1.0 but must finish-and-close before tag).

Earliest possible v1.0 tag date: **2026-07-26** (the day after RFC-002
and RFC-006 close, assuming no re-opens and no modify-and-re-open
cycles). The actual tag will follow once the conformance kit at v1.0
is built (Track B) and the Python 2nd-impl is feature-frozen (Track A).

## Changelog

* **2026-05-26** — initial table populated; all 8 windows opened.
