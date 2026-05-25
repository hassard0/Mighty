# Polish v0.11 — interpretation notes

UX-layer polish slice for the v0.11 work train (parallel-swarm agent
"diagnostic-text polish + tour pages + getting-started flow"). This
file records interpretation calls made while executing the slice so
that reviewers can audit the deltas without re-reading every diff.

## Scope recap

- Owned files only:
  - `crates/mty-diagnostics/src/codes.rs` (explain text only)
  - `docs/tour/*` (all chapters + README)
  - `docs/getting-started.md`
  - `docs/faq.md`
  - `docs/README.md`
  - `dev/history/notes/POLISH_V0_11_NOTES.md` (this file)

- Crate source beyond `codes.rs` is OFF-LIMITS in this slice — that
  belongs to the clippy-cleanup swarm peer.

## Interpretation calls

### IC-1: Source-file extension across docs

The v0.7 rebrand (`stardust → mighty`) changed the source-file
extension from `.sd` to `.mty`. Every shipped example under
`examples/` now ends in `.mty`. The tour and getting-started were
still saying `.sd` in roughly 90 spots — that's the largest single
source of staleness.

Decision: in every documentation snippet, rewrite
- `src/main.sd` → `src/main.mty`
- `examples/NN_*.sd` → `examples/NN_*.mty`
- Inline code fences tagged ` ```sd ` → ` ```mty `

The language identifier on the fence has no syntax-highlighter today
so it is purely a label; renaming it now means we won't have to chase
it down later.

### IC-2: Slice-tagged callouts ("Slice 1 parses these constructs")

Several tour chapters carry "Slice 1 parses these but doesn't enforce
them" callouts that were accurate when written but read as
misinformation today (v0.10 enforces all the things they call out as
"slice 1 only parses"). Decision: rewrite these to either
- delete the slice tag entirely if the feature is now fully shipped,
  or
- restate the slice tag as historical context ("v0.1 parsed-only,
  enforced in v0.5") if the surrounding paragraph still depends on it.

The "v0.7.0-runtime" banners at the top of chapters 6, 8, 11 were
true at v0.7 but everything they call out is now baseline. Decision:
demote to a one-line "Status: shipped in v0.7; runs on the v0.10
executor." note rather than a giant block-quote.

### IC-3: Diagnostic explain text format

The brief asked for a four-line "Cause / Example / Fix / Spec"
format. The existing explain blocks are 2-4 sentence paragraphs and
the test suite asserts they exist (`mty explain MT0001`). Decision:
keep the explain function returning a single `&'static str` (no API
change), but reformat the 14 most-hit codes as multi-line strings
that *render as* four labelled lines. This means the test
(`explain(UNEXPECTED_TOKEN).is_some()`) keeps passing without
modification.

The 14 codes polished:

- MT0001 unexpected_token
- MT0010 expected_item
- MT0011 expected_expr
- MT2001 type_mismatch
- MT2002 unresolved_type (note: spec calls MT2002 "unresolved_type",
  the brief said "unresolved_method" — `unresolved_method` is
  MT2007; polished both because the brief was about high-traffic
  codes)
- MT2007 unknown_method
- MT2021 unresolved_value (the brief's "unresolved_name" maps here;
  MT1001 is also `unresolved_name` but is essentially never
  user-visible because the typeck swallows the same case through
  MT2021)
- MT1001 unresolved_name (polished anyway, low cost)
- MT3001 use_after_move
- MT3004 mut_borrow_while_shared
- MT3005 shared_borrow_while_mut
- MT3006 two_mut_borrows
- MT4001 effect_undeclared
- MT6001 unknown_macro
- MT6004 recursive_macro_too_deep

That's 15 codes, all with the new format.

### IC-4: Spec link target

Every tour chapter linked to `../spec/v0.1.md`. The current
authoritative spec is `../spec/v1.0-rc.md` (`v1.0-RC2`); `v0.1.md`
is a stub. Decision: rewrite spec links to point at `v1.0-rc.md` and
update section numbers where the RC2 renumbered them. Where a
chapter's text references a specific section that hasn't been
renumbered, the link is the only change.

`docs/spec/v0.1-amendments.md` is still authoritative for amendments
A1..A109 (RC2 incorporates them but the amendment file is the
historical record), so links there are left alone.

### IC-5: Getting-started rewrite

The existing getting-started is 187 lines but mostly accurate
shell-by-shell; the v0.10 deltas it's missing are:

- `mty test` (slice merge, was `mty-test`)
- the `mty new app` flag — actually `mty new <NAME>` still
- `mty explain MT####` — wasn't documented
- pre-alpha banner at the top (the spec is RC2 but the toolchain is
  pre-1.0)

Decision: keep the structure, expand to ~290 lines, add a "First
agent → first message" walkthrough that uses the real send/ask
operators, add a `mty explain` section, add a "What's next" footer
that points into the tour by chapter.

### IC-6: FAQ extension

Existing FAQ has 12 Q+A pairs, several stale (e.g. "What works
today (slice 1)"). Decision:

- Rewrite the stale entries to reflect v0.10 status.
- Add new entries covering: A107 SD/MT prefix preservation, A1
  size-suffix grammar, the rebrand mechanics, MSRV (`1.85+`),
  Windows DLL gotcha, macOS LC_BUILD_VERSION fix, "how do I report
  a bug", "what is an early-adopter slot", "is Mighty production
  ready" (no), "can I use it in a hobby project" (yes, with
  warnings), and the difference between an "agent" vs a "task".

Target: ≥15 Q+A pairs in the final file.

### IC-7: docs/README.md tidy

The current index is accurate for slice-1 vintage. Decision:

- Rewrite the "Status snapshot" table to reflect v0.10 reality
  (everything except `pub mty doc` is shipped, in some form).
- Add links to the new docs we expect to land in v0.11 only IF they
  already exist on disk (no broken links).
- Drop the SLICE1.md link (the file still exists under
  `dev/history/` but the index shouldn't surface dev artefacts —
  per the brief).

## Files modified summary

(Filled in at commit time per chunk.)

## Open items for v0.12

- `mty explain` outputs ANSI escapes mid-line on Windows when run
  in cmd.exe (cosmetic; nothing in this slice).
- Tour chapter 7 (send/ask) doesn't demonstrate `@deadline` failure
  with a runtime trap example; nice-to-have but out of scope.
- The FAQ doesn't yet have entries about `mty pkg publish` — that
  surface is RC2-deferred (RFC-004); skip until it ships.
