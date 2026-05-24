# Stardust for VS Code

Syntax highlighting plus the official Stardust Language Server (LSP) for
`.sd` files.

## Features

- Syntax highlighting (keywords, types, strings, numbers, duration/size
  suffixes, doc comments).
- Real-time diagnostics from the Stardust compiler pipeline
  (parse + HIR-lowering + type-check).
- Hover for fns, structs, enums, and other top-level definitions.
- Go-to-definition for top-level item names.
- Document formatting via `sdust fmt`.
- Keyword + def-name completion (semantic / scope-aware completion
  arrives in a follow-up release — see `LSP_PARTIAL.md` in the
  Stardust repo).

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

This produces `stardust-0.2.0.vsix` which you can install with:

```bash
code --install-extension stardust-0.2.0.vsix
```

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `stardust.server.path` | `sdust` | Path to the `sdust` binary. |
| `stardust.trace.server` | `off` | LSP trace level (`off` / `messages` / `verbose`). |

## Known limitations (v0.2)

- Go-to-definition resolves top-level items only (no locals, fields, or
  methods).
- Completion is keyword + top-level-def + (post-dot) built-in method.
  Locals-in-scope and per-receiver semantic completion are deferred.
- Formatting is whole-document (one `TextEdit` per file).
- No workspace folders, code actions, inlay hints, rename, signature
  help, or semantic tokens yet.

## License

Apache-2.0 OR MIT.
