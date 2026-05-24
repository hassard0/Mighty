# Rebrand notes — Stardust → Mighty (v0.7)

This document records the interpretation calls made during the
automated rebrand of the Stardust language to Mighty.

## Canonical decisions (per project owner spec)

| Old | New |
|---|---|
| Language name `Stardust` | `Mighty` |
| CLI binary `sdust` | `mty` |
| Source file extension `.sd` | `.mty` |
| Interface extension `.sdi` | `.mtyi` (no files yet existed) |
| Manifest filename `star.toml` | `mighty.toml` |
| Lockfile `star.lock` | `mighty.lock` |
| IR `SIR` (Stardust IR) | `MtyIR` |
| Diagnostic prefix `SD####` | `MT####` |
| Rowan Language type `Stardust` | `Mighty` |
| Profile dir `.stardust/` | `.mighty/` |
| Crate prefix `sdust-*` | `mty-*` |
| WIT namespace `stardust:caps/*`, `stardust:web/*` | `mty:caps/*`, `mty:web/*` |
| GitHub repo URL | UNCHANGED (`hassard0/stardust`) |
| Edition `2026` | UNCHANGED (it's a year, not a brand) |

## Interpretation calls

### 1. `sdust-sir` crate became `mty-ir` (not `mty-sir`)

The crate name `sdust-sir` was "Stardust IR". Renaming to `mty-sir`
would keep the redundant Stardust-flavor in the name. The crate name
`mty-ir` is cleaner — the IR semantics are still present, just without
the legacy prefix.

Consequences:
- `crates/sdust-sir/` → `crates/mty-ir/`
- `crates/mty-ir/src/sir.rs` → `crates/mty-ir/src/ir.rs`
- `mty_ir::sir::Foo` (cross-crate use) → `mty_ir::ir::Foo`
- Internal `use crate::sir::*` → `use crate::ir::*`
- Internal types dropped the `Sir` prefix:
  - `SirTy` → `IrTy`
  - `SirFnId` → `IrFnId`
  - `SirAgent` → `IrAgent`
  - `AgentSirId` → `AgentIrId`
- CLI dump flag `--sir` → `--ir` (with `alias = "sir"` for back-compat)
- Docs: `docs/internals/sir.md` → `docs/internals/ir.md`

The acronym `SIR` in markdown docs was renamed to `MtyIR`.

### 2. GitHub repo URL `hassard0/stardust` preserved

Per the owner's explicit decision, the GitHub repo name stays
`hassard0/stardust`. All other references to `stardust` in URLs are
swept to `mighty`, but the repo URL is sentinel-preserved. The spec
notes this in §1 Naming, and the project README bridges the two.

### 3. Back-compat for legacy SD-prefixed diagnostic codes

The `mty explain` CLI accepts both `MT0001` (canonical) and `SD0001`
(legacy). This keeps any v0.6-era bug reports and external docs
navigable.

### 4. Back-compat for the `--sir` dump flag

`mty dump --sir` still works (clap alias to `--ir`). Documented in
the help text.

### 5. CLI binary version string

`mty --version` reports `mty 0.1.0` (workspace version unchanged).
Bumped the project tag to `v0.7.0-rebrand` to mark the boundary.

### 6. VS Code extension version bumped 0.5.0 → 0.7.0

Aligns with the language rebrand version. The publisher field changed
`stardust-lang` → `mighty-lang` (matches the new package name `mighty`).
Old `stardust-0.5.0.vsix` artifact removed; rebuild via
`npx vsce package` to produce `mighty-0.7.0.vsix`.

### 7. Test fixtures with embedded "Stardust" string literals

`examples/01_hello.mty`: was `log("hello, Stardust")`; now
`log("hello, Mighty")`. The matching test
(`mty-driver::interp_runnable::hello_world_prints`) was updated to
expect `"hello, Mighty"`.

`demos/03_extract_tool`: the breach text contained the literal token
`Stardust` to demonstrate the extractor; the source + expected-output
were both updated to `Mighty`.

### 8. WIT package names

`stardust:caps/*` and `stardust:web/*` became `mty:caps/*` and
`mty:web/*`. The `wasi:*` packages were untouched (they are upstream
WASI types, not Mighty's).

The `dom-shim.js` host shim for demo 02 was updated to import
`mty:web/dom` instead of `stardust:web/dom`.

### 9. Renamed test/binary source files (in addition to crate dirs)

- `crates/mty-bench/src/bin/sdust-bench-runner.rs` → `mty-bench-runner.rs`
- `crates/mty-stdlib/src/bin/sdust-test.rs` → `mty-test.rs`
- `crates/mty-stdlib/tests/sdust_run_demo.rs` → `mty_run_demo.rs`
- `crates/mty-borrow/tests/sd3009_move_via_ref.rs` → `mt3009_move_via_ref.rs`
- `crates/mty-macros/tests/unknown_macro_sd6001.rs` → `unknown_macro_mt6001.rs`

The `[[bin]] name = "sdust-bench-runner"` and `[[bin]] name = "sdust-test"`
in respective Cargo.tomls were updated to `mty-bench-runner` and `mty-test`.

### 10. Documentation code-fence language

The docstring code-fence language went from ` ```sd ` (or ` ```stardust `)
to ` ```mty `. The `mty-doc` extractor was updated to recognize `mty` and
`mighty` as the Mighty language identifiers; `sd` and `stardust` are no
longer recognized.

### 11. Edition `"2026"`

Per spec, edition is a calendar year and stays as `"2026"` — it
identifies the language-syntax revision, not the brand. Mighty's
edition policy follows the same year-based scheme as Rust.

### 12. Benchmark comparator directories

`benches/<bench-name>/stardust/` → `benches/<bench-name>/mighty/`.
The sister directories `rust/`, `cpp/`, `go/`, `cpp-asio/`, etc. are
external comparators and were untouched.

## What was explicitly preserved

- `https://github.com/hassard0/stardust` (repo URL)
- `edition = "2026"` (calendar year, not brand)
- `wasi:*` WIT imports (upstream WASI namespace)
- The `pkg.stardust.dev` registry URL constant in `lib.rs` (not in
  scope for this rebrand — only the brand identifiers were swept)
- External Rust comparator crates under `benches/*/rust/`
- The publisher's `node_modules` and `out/` build artefacts under
  `editor/vscode/`

## Things that need follow-up after merge

1. Domain — `pkg.stardust.dev` is still hardcoded in
   `crates/mty-pkg/src/lockfile.rs::DEFAULT_REGISTRY`. If the project
   acquires `pkg.mighty.dev` (or similar), swap the constant + emit
   migration helpers for in-the-wild `mighty.lock` files.
2. VS Code marketplace listing — the published extension (if any) at
   `stardust-lang.stardust` should be migrated to `mighty-lang.mighty`
   or republished. The new VSIX builds as `mighty-0.7.0.vsix`.
3. `stardust_language_spec_v0_1.md` (external) — the project owner's
   working copy of the full spec lives outside the repo. Update its
   title page when convenient; the in-repo `docs/spec/v0.1.md` now
   references it as `mighty_language_spec_v0_1.md`.
4. `RELEASE-v0.7.md` — not yet authored; the rebrand commits will be
   tagged `v0.7.0-rebrand` and a release note can be drafted from this
   doc plus `RENAME_LOG.md` after merge.
