# Stardust for VS Code

Syntax highlighting plus the official Stardust Language Server (LSP) for
`.sd` files.

## Features (v0.5)

- Syntax highlighting (keywords, types, strings, numbers, duration/size
  suffixes, doc comments).
- Real-time diagnostics from the Stardust compiler pipeline
  (parse + HIR-lowering + type-check).
- Hover for fns, structs, enums, and other top-level definitions.
- Go-to-definition for top-level item names.
- Document formatting via `sdust fmt`.
- **Completion** — keywords + top-level defs + locals in scope +
  receiver-aware methods/fields after `.`.
- **Semantic tokens** — overlays on the TextMate grammar to distinguish
  user-defined types from primitives, fns from variables, parameters
  from locals, etc.
- **Rename (F2)** — single-file rename for locals and top-level items
  with a prepareRename preview.
- **Inlay hints** — `: T` annotations on `let` bindings and fn
  parameters whose type was inferred. Off by default; toggle via
  `stardust.inlayHints.enable`.
- **Code actions** — quick fixes for `unresolved value` (MT2021),
  `unresolved type` (MT2002), `use after move` (MT3001), and
  `effect undeclared` (MT4001).
- **Signature help** — pops up on `(` and `,` inside call sites, with
  the active parameter highlighted.

## Requirements

A `sdust` binary on your `PATH`, or configure `stardust.server.path` to
point at one. The extension spawns `sdust lsp` over stdio.

```bash
cd /path/to/stardust
cargo install --path crates/sdust-cli
```

## Build the extension locally

```bash
cd editor/vscode
npm install
npm run compile
npx vsce package
```

This produces `stardust-0.5.0.vsix` which you can install with:

```bash
code --install-extension stardust-0.5.0.vsix
```

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `stardust.server.path` | `sdust` | Path to the `sdust` binary. |
| `stardust.trace.server` | `off` | LSP trace level (`off` / `messages` / `verbose`). |
| `stardust.inlayHints.enable` | `false` | Show inferred-type hints next to `let` bindings and fn parameters. |
| `stardust.semanticTokens.enable` | `true` | Use the LSP's semantic-token classifier to highlight identifiers more precisely than the TextMate grammar can. |

## Commands

| Command | Description |
| --- | --- |
| `Stardust: Restart Language Server` | Stop and restart `sdust lsp`. Useful after changing `stardust.server.path` or recovering from a server crash. |

## Keybindings

| Key | Command |
| --- | --- |
| `F2` | Rename symbol under cursor (Stardust files only). |

## Known limitations (v0.5)

- Rename is single-file. Cross-file rename arrives once the LSP builds
  a workspace-wide resolve map.
- Go-to-definition still resolves top-level items only (no locals,
  fields, or methods).
- Borrow-check diagnostics live in `sdust check` from the CLI, not in
  the editor pipeline (latency).
- Inlay hints don't yet cover closure parameters or argument names.
- Signature help doesn't yet disambiguate trait-method overloads by
  receiver type; it lists every candidate.

## License

Apache-2.0 OR MIT.
