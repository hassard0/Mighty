# sdust lsp

Run the Stardust Language Server over stdio. Used by editor plugins
(VS Code, Neovim, Helix, Emacs, etc.) to provide live diagnostics,
hover, go-to-definition, formatting, completion, semantic tokens,
rename, inlay hints, code actions, and signature help for `.sd` files.

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
| `definitionProvider` | `true` | Top-level item names only (deferred). |
| `completionProvider` | `{ triggerCharacters: [".", ":"] }` | Keywords + def names + locals + receiver-aware methods + built-in methods. |
| `documentFormattingProvider` | `true` | Whole-document via `sdust fmt`. |
| `semanticTokensProvider` | full + range, legend below | v0.5: 14 types × 3 modifiers. |
| `renameProvider` | `{ prepareProvider: true }` | Single-file scope (v0.5). |
| `codeActionProvider` | `{ codeActionKinds: ["quickfix"] }` | MT2021 / MT2002 / MT3001 / MT4001 fixes. |
| `signatureHelpProvider` | `{ triggerCharacters: ["(", ","] }` | Call + method-call sites. |
| `inlayHintProvider` | `{ resolveProvider: false }` | `let` + fn-param type hints. |
| `workspace.workspaceFolders` | `{ supported: true, changeNotifications: true }` | Event surface; per-file analysis. |

### Semantic token legend (v0.5)

| index | type | index | type | index | type |
|---|---|---|---|---|---|
| 0 | `keyword` | 5 | `string` | 10 | `enumMember` |
| 1 | `type` | 6 | `number` | 11 | `typeParameter` |
| 2 | `function` | 7 | `comment` | 12 | `macro` |
| 3 | `variable` | 8 | `operator` | 13 | `property` |
| 4 | `parameter` | 9 | `namespace` | | |

Modifier bits: `0 = declaration`, `1 = readonly`, `2 = defaultLibrary`.

## In-scope methods (v0.5)

- `initialize` / `initialized` / `shutdown` / `exit`
- `textDocument/didOpen`
- `textDocument/didChange` (incremental)
- `textDocument/didClose`
- `textDocument/publishDiagnostics` (server → client, on every change)
- `textDocument/hover`
- `textDocument/definition`
- `textDocument/formatting`
- `textDocument/completion`
- `textDocument/semanticTokens/full`
- `textDocument/semanticTokens/range`
- `textDocument/rename`
- `textDocument/prepareRename`
- `textDocument/inlayHint`
- `textDocument/codeAction`
- `textDocument/signatureHelp`
- `workspace/didChangeWorkspaceFolders`
- `workspace/didChangeWatchedFiles`

## Out-of-scope (still deferred)

- Cross-file rename / cross-file go-to-def (workspace folders are
  observed but each file is still analyzed independently).
- Borrow-check diagnostics in the editor — `sdust check` from the CLI
  still runs the full borrow checker.
- Call hierarchy / type hierarchy.
- Signature-help overload disambiguation by receiver type (we list
  every candidate today).
- Closure parameter inlay hints, argument-name inlay hints.
- `inlayHint/resolve` (we don't yet attach lazy data).

## Editor setup

### VS Code

Install the `editor/vscode` extension from the Stardust repo:

```bash
cd editor/vscode
npm install
npm run compile
npx vsce package
code --install-extension stardust-0.5.0.vsix
```

By default the extension launches whatever `sdust` is on `PATH`.
Override via the `stardust.server.path` setting. Inlay hints are off
by default (`stardust.inlayHints.enable = false`); enable them in your
settings if you want them.

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
