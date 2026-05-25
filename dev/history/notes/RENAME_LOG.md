# Rename log — Stardust → Mighty (v0.7)

A complete log of every `(old, new)` pair applied during the rebrand,
for archival + future reference.

## 1. Crate directory renames (21 total)

| Old | New |
|---|---|
| `crates/sdust-ast` | `crates/mty-ast` |
| `crates/sdust-bench` | `crates/mty-bench` |
| `crates/sdust-borrow` | `crates/mty-borrow` |
| `crates/sdust-cli` | `crates/mty-cli` |
| `crates/sdust-codegen-cranelift` | `crates/mty-codegen-cranelift` |
| `crates/sdust-codegen-llvm` | `crates/mty-codegen-llvm` |
| `crates/sdust-codegen-wasm` | `crates/mty-codegen-wasm` |
| `crates/sdust-debuginfo` | `crates/mty-debuginfo` |
| `crates/sdust-diagnostics` | `crates/mty-diagnostics` |
| `crates/sdust-doc` | `crates/mty-doc` |
| `crates/sdust-driver` | `crates/mty-driver` |
| `crates/sdust-fmt` | `crates/mty-fmt` |
| `crates/sdust-hir` | `crates/mty-hir` |
| `crates/sdust-lsp` | `crates/mty-lsp` |
| `crates/sdust-macros` | `crates/mty-macros` |
| `crates/sdust-pkg` | `crates/mty-pkg` |
| `crates/sdust-runtime` | `crates/mty-runtime` |
| `crates/sdust-sir` | `crates/mty-ir` *(special: drops `s` prefix)* |
| `crates/sdust-stdlib` | `crates/mty-stdlib` |
| `crates/sdust-syntax` | `crates/mty-syntax` |
| `crates/sdust-types` | `crates/mty-types` |

## 2. Bin renames

| Old | New |
|---|---|
| `sdust` (CLI binary) | `mty` |
| `sdust-bench-runner` | `mty-bench-runner` |
| `sdust-test` | `mty-test` |

## 3. Source file renames

| Old | New |
|---|---|
| `crates/mty-ir/src/sir.rs` | `crates/mty-ir/src/ir.rs` |
| `crates/mty-bench/src/bin/sdust-bench-runner.rs` | `mty-bench-runner.rs` |
| `crates/mty-stdlib/src/bin/sdust-test.rs` | `mty-test.rs` |
| `crates/mty-stdlib/tests/sdust_run_demo.rs` | `mty_run_demo.rs` |
| `crates/mty-borrow/tests/sd3009_move_via_ref.rs` | `mt3009_move_via_ref.rs` |
| `crates/mty-macros/tests/unknown_macro_sd6001.rs` | `unknown_macro_mt6001.rs` |
| `editor/vscode/syntaxes/stardust.tmLanguage.json` | `mighty.tmLanguage.json` |
| `docs/internals/sir.md` | `docs/internals/ir.md` |
| `docs/reference/cli/sdust*.md` (11 files) | `mty*.md` |

## 4. Identifier renames

| Old | New |
|---|---|
| `SirTy` | `IrTy` |
| `SirFnId` | `IrFnId` |
| `SirAgent` | `IrAgent` |
| `AgentSirId` | `AgentIrId` |
| `pub enum Stardust {}` (Rowan Language) | `pub enum Mighty {}` |

## 5. Diagnostic codes (every numeric value preserved, prefix swapped)

| Old | New |
|---|---|
| `SD0001`..`SD0099` (lex/parse) | `MT0001`..`MT0099` |
| `SD1001`..`SD1099` (HIR) | `MT1001`..`MT1099` |
| `SD2001`..`SD2099` (typeck) | `MT2001`..`MT2099` |
| `SD3001`..`SD3099` (borrowck) | `MT3001`..`MT3099` |
| `SD4001`..`SD4099` (effects/caps/protocols) | `MT4001`..`MT4099` |
| `SD5001`..`SD5099` (runtime) | `MT5001`..`MT5099` |
| `SD6001`..`SD6099` (macros) | `MT6001`..`MT6099` |
| `SD8001`..`SD8010` (codegen traps) | `MT8001`..`MT8010` |

The `DiagCode::as_str()` format string went from `"SD{:04}"` to
`"MT{:04}"`. `mty explain` accepts both `MT`/`mt` (canonical) and
`SD`/`sd` (legacy) prefixes for back-compat.

## 6. File extensions (143 files)

| Old | New |
|---|---|
| `*.sd` (Mighty source) | `*.mty` |
| `*.sdi` (Mighty interface, no files yet) | `*.mtyi` |

## 7. Manifest filenames

| Old | New |
|---|---|
| `star.toml` (4 files: root + 3 demos) | `mighty.toml` |
| `star.lock` (loader path constant) | `mighty.lock` |
| `MANIFEST_NAME = "star.toml"` | `MANIFEST_NAME = "mighty.toml"` |

## 8. Profile / cache directories (in source-code constants)

| Old | New |
|---|---|
| `.stardust/pkgs/` | `.mighty/pkgs/` |
| `.stardust/registry/` | `.mighty/registry/` |
| `.stardust/publish/` | `.mighty/publish/` |
| `~/.config/sdust/auth.toml` | `~/.config/mighty/auth.toml` |

## 9. WIT namespaces

| Old | New |
|---|---|
| `stardust:caps/<family>` | `mty:caps/<family>` |
| `stardust:web/log` | `mty:web/log` |
| `stardust:web/dom` | `mty:web/dom` |
| `package stardust:caps` | `package mty:caps` |
| `package stardust:web` | `package mty:web` |

## 10. Benchmark comparator directories

| Old | New |
|---|---|
| `benches/agent_send_latency/stardust/` | `benches/agent_send_latency/mighty/` |
| `benches/compile_to_native/stardust/` | `benches/compile_to_native/mighty/` |
| `benches/http_server_throughput/stardust/` | `benches/http_server_throughput/mighty/` |
| `benches/mailbox_throughput/stardust/` | `benches/mailbox_throughput/mighty/` |
| `benches/parse_throughput/stardust/` | `benches/parse_throughput/mighty/` |
| `benches/wasm_size/stardust/` | `benches/wasm_size/mighty/` |

## 11. CLI flag renames (with back-compat aliases)

| Old | New | Back-compat |
|---|---|---|
| `mty dump --sir` | `mty dump --ir` | clap `alias = "sir"` keeps `--sir` working |
| `mty explain SD0001` | `mty explain MT0001` | accepts `SD`/`sd` prefix too |

## 12. VS Code extension (editor/vscode/)

| Field | Old | New |
|---|---|---|
| `package.json:name` | `stardust` | `mighty` |
| `package.json:displayName` | `Stardust` | `Mighty` |
| `package.json:publisher` | `stardust-lang` | `mighty-lang` |
| `package.json:version` | `0.5.0` | `0.7.0` |
| Language id | `stardust` | `mighty` |
| Language alias | `Stardust`, `sdust` | `Mighty`, `mty` |
| File extension | `.sd` | `.mty` |
| Scope name | `source.stardust` | `source.mty` |
| Server command | `sdust` | `mty` |
| Config namespace | `stardust.*` | `mighty.*` |
| Commands prefix | `stardust.restartServer` | `mighty.restartServer` |
| Removed artifact | `stardust-0.5.0.vsix` | (re-package as `mighty-0.7.0.vsix`) |

## 13. Editor configuration

The `editor/vscode/language-configuration.json` had no
language-name string literals — it was untouched.

## 14. Doc-comment code fences

| Old | New |
|---|---|
| ` ```sd ` | ` ```mty ` |
| ` ```stardust ` | ` ```mty ` |

The `mty-doc::extract` recognizer now matches `mty` and `mighty`.
The legacy `sd` and `stardust` identifiers are no longer recognized
as Mighty code — they will be treated as opaque language hints by
the extractor.
