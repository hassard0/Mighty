# Mighty v0.7 — Release Notes

**Tag:** `v0.7.0-rebrand`
**Date:** 2026-05-24
**Status:** SHIPPED — naming-only release. The Stardust language has
been rebranded to **Mighty**. This release contains no feature
changes; every test passes byte-for-byte against v0.6.0 (885 passed,
0 failed, 2 ignored). What changes is identifiers, file extensions,
and the user-facing brand surface.

If you were on v0.6.0, the upgrade is a one-time identifier swap.
All v0.6 behavior, performance, soundness, and binaries are
preserved.

## TL;DR — what to change in your code

| You had | You now have |
|---|---|
| `sdust check src/main.sd` | `mty check src/main.mty` |
| `sdust new app` | `mty new app` |
| `sdust fmt` / `sdust run` / `sdust build` / `sdust lsp` / `sdust pkg` / `sdust doc` | replace `sdust` with `mty` |
| `star.toml` | `mighty.toml` |
| `star.lock` | `mighty.lock` |
| `.sd` source files | `.mty` source files |
| `.stardust/` cache dir | `.mighty/` |
| `SD0001`..`SD8010` diagnostic codes | `MT0001`..`MT8010` (the `SD` prefix is still accepted by `mty explain` for legacy bug reports) |
| `extern crate sdust_syntax;` | `extern crate mty_syntax;` |
| `use sdust_*::...` | `use mty_*::...` |
| `mty dump --sir` (was `sdust dump --sir`) | `mty dump --ir` (the `--sir` alias still works) |
| `pub enum Stardust {}` (rowan Language type) | `pub enum Mighty {}` |
| WIT imports `stardust:caps/*`, `stardust:web/*` | `mty:caps/*`, `mty:web/*` |
| `editor/vscode/stardust-0.5.0.vsix` | `editor/vscode/mighty-0.7.0.vsix` (rebuild via `npx vsce package`) |

## Why the rebrand

The project rebrands from Stardust to Mighty as it approaches the
v1.0 release candidate. The new name is shorter, easier to say, and
distinct from the dozens of "Stardust"-named software packages on
crates.io / npm / GitHub. The original spec — preserved at
`stardust_language_spec_v0_1.md` in your Downloads — remains the
normative reference for the language semantics; only the brand
identifiers move.

## What's NOT changed

- **GitHub repo URL** stays `https://github.com/hassard0/stardust`.
  Renaming the GitHub repo would break every existing clone,
  bookmark, and external link; the project name and the repo name
  are decoupled (see the README for the bridging note).
- **Edition `"2026"`** — the language edition is a calendar year, not
  a brand identifier. Mighty's edition policy follows the same
  year-based scheme as Rust.
- **`wasi:*` WIT imports** — those are upstream WASI types, not
  Mighty's.
- **All test counts and benchmark numbers** — v0.7 is byte-for-byte
  behaviorally identical to v0.6.0. 885 tests pass.

## Crate renames (21 crates)

Every workspace crate `sdust-*` → `mty-*`:

```
sdust-syntax             → mty-syntax
sdust-ast                → mty-ast
sdust-diagnostics        → mty-diagnostics
sdust-hir                → mty-hir
sdust-types              → mty-types
sdust-borrow             → mty-borrow
sdust-sir                → mty-ir            (* dropped redundant `s` prefix)
sdust-runtime            → mty-runtime
sdust-codegen-cranelift  → mty-codegen-cranelift
sdust-codegen-wasm       → mty-codegen-wasm
sdust-codegen-llvm       → mty-codegen-llvm
sdust-fmt                → mty-fmt
sdust-lsp                → mty-lsp
sdust-pkg                → mty-pkg
sdust-doc                → mty-doc
sdust-stdlib             → mty-stdlib
sdust-debuginfo          → mty-debuginfo
sdust-macros             → mty-macros
sdust-bench              → mty-bench
sdust-driver             → mty-driver
sdust-cli                → mty-cli
```

The `sdust-sir` crate became `mty-ir` (not `mty-sir`) — the old name
carried the redundant "Stardust IR" flavor; `mty-ir` is cleaner.
Internal types dropped the `Sir` prefix:

- `SirTy` → `IrTy`
- `SirFnId` → `IrFnId`
- `SirAgent` → `IrAgent`
- `AgentSirId` → `AgentIrId`

The acronym `SIR` in markdown docs became `MtyIR`.

## Diagnostic code prefix

`SD####` → `MT####`. Every diagnostic code keeps its number; only
the two-letter prefix moves. The `mty explain` CLI accepts BOTH
`MT0001` (canonical) and `SD0001` (legacy) for at least one major
version, so existing bug reports and external docs remain navigable.

## Backwards-compat aliases

For one major version (i.e. until v1.0 or v2.0), the following
aliases stay live:

- `mty dump --sir` aliases `--ir`
- `mty explain SD####` accepts the legacy `SD` prefix and maps to the
  same `MT####` entry
- The `--legacy-interp` flag (from v0.6) is unchanged

These aliases will be removed in a future major release; tracked as
A45 in the consolidated spec.

## Stats

| | v0.6.0 | v0.7.0-rebrand | Delta |
|---|---|---|---|
| Workspace crates | 20 (`sdust-*`) | 20 (`mty-*`) | renamed |
| Source files | 168 Rust + 143 `.sd` | 168 Rust + 143 `.mty` | renamed |
| Tests passing | 885 | 885 | 0 |
| Tests failing | 0 | 0 | 0 |
| Diagnostic codes | 65+ (SD prefix) | 65+ (MT prefix + SD alias) | rebadged |
| Examples passing | 20/20 | 20/20 | 0 |
| Demos passing | 3/3 | 3/3 | 0 |
| Self-host lexer | full diff | full diff | unchanged |
| Self-host parser | examples 01–05 | examples 01–05 | unchanged |
| Lines changed | — | 844 files | rebrand sweep |
| Commits | — | 4 rebrand commits | A→D |

## Migration steps for existing projects

If you have a v0.6 Mighty package:

1. **Rename `star.toml` → `mighty.toml`**.
2. **Rename source files `*.sd` → `*.mty`**. A one-liner:
   ```bash
   find . -name "*.sd" -not -path "./target/*" | while read f; do
     git mv "$f" "${f%.sd}.mty"
   done
   ```
3. **Update CLI invocations** in your scripts, CI workflows, and
   Dockerfiles: `sdust` → `mty`.
4. **Update Rust dependents** if you import any Mighty crates:
   `sdust-*` → `mty-*` in your `Cargo.toml`.
5. **Update docs** that say "Stardust" — only your project's
   user-facing docs need this; the Mighty toolchain accepts the
   legacy spellings via the aliases listed above.

After these steps, `cargo install --path crates/mty-cli` on the v0.7
repo and then `mty check src/main.mty` from your project. Behavior
is identical to v0.6.

## Post-rebrand follow-ups (v0.7.x or v0.8)

Documented in `REBRAND_NOTES.md`:

1. **VS Code marketplace listing** — the published extension (if
   any) at `stardust-lang.stardust` should be migrated to
   `mighty-lang.mighty` or republished. The new VSIX builds as
   `mighty-0.7.0.vsix`.
2. **Package registry domain** — `pkg.stardust.dev` is still
   hardcoded as the default registry constant in `mty-pkg`. If the
   project acquires `pkg.mighty.dev` (or similar), swap the constant
   and emit migration helpers for existing `mighty.lock` files.
3. **External docs / blog posts** — anything pointing at `sdust` CLI
   invocations or `.sd` files should be re-recorded. The aliases
   above mean old docs work but aren't idiomatic.
4. **Runtime ABI symbol names** — the LLVM codegen still references
   `stardust_runtime_*` external symbols for the runtime ABI (log,
   panic, send, ask, spawn, arena_*, budget_*, extern_call). These
   are real misses the rebrand sweep didn't catch — being addressed
   in the v0.8 post-swarm cleanup pass. Cranelift and Wasm codegen
   paths are unaffected.
5. **DWARF producer string** — debug info emits
   `producer: "stardust-0.2"`; will be updated to `"mighty-X.Y"` in
   the v0.8 cleanup.
6. **`mty-bench` fixture name** — `stardust_10kloc()` fixture
   function in `mty-bench` will be renamed to `mty_10kloc()` in the
   v0.8 cleanup.
7. **Insta snapshot headers** — `mty-hir/tests/snapshots/*.snap`
   files still have `source: crates/sdust-hir/tests/...` lines; need
   regeneration via `INSTA_UPDATE=always`.
8. **`mty-doc` template comments** — `templates/style.css` and
   `templates/search.js` still mention `sdust-doc`; cosmetic only.

These were missed because the rebrand agent's phase-4 pattern matched
the abbreviated `sdust_` form but not the full `stardust_` form. The
v0.8 cleanup pass closes them in one commit.

## Why no feature work in v0.7

The rebrand is intentionally a standalone release. Conflating a
rebrand with feature changes makes both harder to verify: every test
failure would be ambiguous (rebrand miss? feature regression?). By
shipping the rebrand alone with a 0-test-delta guarantee, v0.7
becomes a clean reference point for downstream consumers and a
stable base for v0.8's feature work.

Feature work resumes in v0.8 with the v0.5 loose-ends closure,
self-host HIR, performance optimizations targeting v0.6 benchmark
losses, and the v1.0-RC spec consolidation.

## Acknowledgments

The rebrand was executed in a single 6-hour autonomous agent run
covering 18 phases: crate dir renames → workspace manifest →
per-crate Cargo.toml → source mass-text → diag code prefix → file
extension rename → manifest filename → CLI text → docs → VS Code
extension → GitHub Actions → issue templates → WIT interfaces →
selfhost → spec amendments → verification → spec naming → tag.
844 files changed, 4 logical commits, 0 test regressions.

See `REBRAND_NOTES.md` for full interpretation calls and
`RENAME_LOG.md` for the complete (old, new) pair manifest.
