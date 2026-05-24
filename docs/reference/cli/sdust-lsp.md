# sdust lsp

Run the Stardust Language Server over stdio. Used by editor plugins
(VS Code, Neovim, Helix, Emacs, etc.) to provide live diagnostics,
hover, go-to-definition, formatting, and completion for `.sd` files.

## Synopsis

```
sdust lsp
```

No flags; no arguments. The process reads JSON-RPC framed messages from
stdin and writes responses + notifications to stdout, per the LSP 3.17
specification. Trace / log output goes to the LSP `window/logMessage`
channel (not stderr) so it is editor-visible.

## Capabilities advertised

The server reports the following capabilities in its
`initialize` response:

| Capability | Value | Notes |
|---|---|---|
| `textDocumentSync` | `Incremental` | Per-edit range patches; full sync also accepted. |
| `hoverProvider` | `true` | Markdown body. |
| `definitionProvider` | `true` | Top-level item names only (v0.2). |
| `completionProvider` | `{ triggerCharacters: [".", ":"] }` | Keywords + def names + (post-dot) built-in methods. |
| `documentFormattingProvider` | `true` | Whole-document via `sdust fmt`. |

## In-scope methods (v0.2 MVP)

- `initialize` / `initialized` / `shutdown` / `exit`
- `textDocument/didOpen`
- `textDocument/didChange` (incremental)
- `textDocument/didClose`
- `textDocument/publishDiagnostics` (server → client, on every change)
- `textDocument/hover`
- `textDocument/definition`
- `textDocument/formatting`
- `textDocument/completion`

## Out-of-scope (v0.2)

Documented as v0.2 amendments — to be addressed in a follow-up release:

- Workspace folders + multi-file resolution
- Code actions (`textDocument/codeAction`)
- Inlay hints (`textDocument/inlayHint`)
- Rename (`textDocument/rename`)
- Signature help (`textDocument/signatureHelp`)
- Semantic tokens (`textDocument/semanticTokens`)
- Borrow-check diagnostics (parse + lower + type-check are surfaced;
  borrow check is currently CLI-only via `sdust check`)
- Locals-in-scope / per-receiver semantic completion

## Editor setup

### VS Code

Install the `editor/vscode` extension from the Stardust repo:

```bash
cd editor/vscode
npm install
npm run compile
npx vsce package
code --install-extension stardust-0.2.0.vsix
```

By default the extension launches whatever `sdust` is on `PATH`.
Override via the `stardust.server.path` setting.

### Neovim (nvim-lspconfig)

```lua
local lspconfig = require('lspconfig')
local configs = require('lspconfig.configs')

if not configs.stardust then
  configs.stardust = {
    default_config = {
      cmd = { "sdust", "lsp" },
      filetypes = { "stardust" },
      root_dir = lspconfig.util.root_pattern("star.toml", ".git"),
      settings = {},
    },
  }
end

lspconfig.stardust.setup({})
```

Plus a filetype mapping:

```vim
autocmd BufNewFile,BufRead *.sd set filetype=stardust
```

### Helix

```toml
# ~/.config/helix/languages.toml
[[language]]
name = "stardust"
scope = "source.stardust"
file-types = ["sd"]
comment-token = "//"
language-servers = ["sdust"]
indent = { tab-width = 4, unit = "    " }

[language-server.sdust]
command = "sdust"
args = ["lsp"]
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Server shut down cleanly. |
| `1` | Failed to start (e.g. tokio runtime build failed). |

## Examples

Run the server directly (for transport debugging) and feed it a hand-rolled
`initialize`:

```bash
echo 'Content-Length: 99\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}' \
  | sdust lsp
```

You should see `initialize` reply with the capability set above.

## Implementation notes

The server lives in `crates/sdust-lsp` and is built on
[`tower-lsp`](https://crates.io/crates/tower-lsp) 0.20. Per-document
state (source text, line index, parsed CST, lowered HIR, type-check
side tables) is cached in a `DashMap<Url, Arc<DocAnalysis>>` and
re-run on every change. See `docs/internals/lsp.md` for the full
architecture.
