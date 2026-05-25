# Development history

This directory contains development artifacts preserved for posterity.
**None of this is required for using Mighty** — end users and most
contributors can ignore it. Look here only if you want a paper trail
of how the toolchain got built slice-by-slice.

## Layout

- [`history/slices/`](history/slices/) — per-slice implementation
  notes (`SLICE*.md`), one per scoped milestone slice from
  v0.1's eight ladder slices through v0.9.
- [`history/releases/`](history/releases/) — the full per-release
  notes (`RELEASE-v0.1.md` .. `RELEASE-v0.9.md`). The repo-root
  [`CHANGELOG.md`](../CHANGELOG.md) summarises each; this directory
  holds the unabridged versions.
- [`history/notes/`](history/notes/) — agent-level working notes
  per workstream (e.g. `EFFECTS_V0_3_NOTES.md`,
  `SELFHOST_PARSER_V0_6_NOTES.md`, plus `REBRAND_NOTES.md` +
  `RENAME_LOG.md` from the Stardust → Mighty rename).
- [`history/superpowers/docs/`](history/superpowers/docs/) — the
  original brainstorming-skill plan + spec docs that guided early
  slice development (`plans/` + `specs/`). Once a slice landed,
  these became read-only historical references.

## Why keep all this?

The slice-by-slice spec-driven build worked because each slice was
scoped from spec §31, designed in `superpowers/specs/`, planned in
`superpowers/plans/`, executed mostly by subagent swarms, and gated
by review checkpoints. The historical artifacts let future readers
reconstruct *why* a given subsystem looks the way it does.

They were just cluttering the repo root.
