# Mighty for VS Code (legacy v0.5 extension)

> **Superseded by [`tools/vscode/`](../../tools/vscode/README.md).**
> This directory holds the original v0.5 prototype extension. The
> v0.31+ production extension (with cost CodeLens, DAP debugger,
> quick-fix lightbulb, cost side-panel, etc.) lives at
> `tools/vscode/` and is the one published to the marketplace.
> Keep this directory only for historical reference; new work
> happens in `tools/vscode/`.

The notes below describe the v0.5 surface as shipped.

## Features (v0.5)

- Syntax highlighting (keywords, types, strings, numbers, duration/size
  suffixes, doc comments).
- Real-time diagnostics from the Mighty compiler pipeline
  (parse + HIR-lowering + type-check).
- Hover for fns, structs, enums, and other top-level definitions.
- Go-to-definition for top-level item names.
- Document formatting via `mty fmt`.
- **Completion** — keywords + top-level defs + locals in scope +
  receiver-aware methods/fields after `.`.
- **Semantic tokens** — overlays on the TextMate grammar to distinguish
  user-defined types from primitives, fns from variables, parameters
  from locals, etc.
- **Rename (F2)** — single-file rename for locals and top-level items
  with a prepareRename preview.
- **Inlay hints** — `: T` annotations on `let` bindings and fn
  parameters whose type was inferred. Off by default; toggle via
  `mighty.inlayHints.enable`.
- **Code actions** — quick fixes for `unresolved value` (MT2021),
  `unresolved type` (MT2002), `use after move` (MT3001), and
  `effect undeclared` (MT4001).
- **Signature help** — pops up on `(` and `,` inside call sites, with
  the active parameter highlighted.

## Requirements

A `mty` binary on your `PATH`, or configure `mighty.server.path` to
point at one. The extension spawns `mty lsp` over stdio.

```bash
cd /path/to/mighty
cargo install --path crates/mty-cli
```

## Build the extension locally

```bash
cd editor/vscode
npm install
npm run compile
npx vsce package
```

This produces `mighty-0.5.0.vsix` which you can install with:

```bash
code --install-extension mighty-0.5.0.vsix
```

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `mighty.server.path` | `mty` | Path to the `mty` binary. |
| `mighty.trace.server` | `off` | LSP trace level (`off` / `messages` / `verbose`). |
| `mighty.inlayHints.enable` | `false` | Show inferred-type hints next to `let` bindings and fn parameters. |
| `mighty.semanticTokens.enable` | `true` | Use the LSP's semantic-token classifier to highlight identifiers more precisely than the TextMate grammar can. |

## Commands

| Command | Description |
| --- | --- |
| `Mighty: Restart Language Server` | Stop and restart `mty lsp`. Useful after changing `mighty.server.path` or recovering from a server crash. |

## Keybindings

| Key | Command |
| --- | --- |
| `F2` | Rename symbol under cursor (Mighty files only). |

## Known limitations (v0.5)

- Rename is single-file. Cross-file rename arrives once the LSP builds
  a workspace-wide resolve map.
- Go-to-definition still resolves top-level items only (no locals,
  fields, or methods).
- Borrow-check diagnostics live in `mty check` from the CLI, not in
  the editor pipeline (latency).
- Inlay hints don't yet cover closure parameters or argument names.
- Signature help doesn't yet disambiguate trait-method overloads by
  receiver type; it lists every candidate.

## License

MIT — see [LICENSE](../../LICENSE) in the repository root.
