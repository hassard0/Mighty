# Mighty Language — VS Code extension

First-class [Mighty](https://github.com/hassard0/Mighty) support for VS Code.
Shipped as the v0.31 Track 2 deliverable; lives in-tree at `tools/vscode/`
so that grammar, LSP wiring, and CLI commands evolve in lockstep with the
language itself.

## What's in the box

| Surface | What you get |
| --- | --- |
| Syntax highlighting | Hand-rolled TextMate grammar covering keywords (hard + soft), agents, protocols, decorators (`@tool`, `@computer_use`, `@eval`, …), capabilities, effect rows, taint markers, numeric literals (incl. `Duration` / `Size`), HTML literals, and raw strings. |
| LSP client | Spawns `mty lsp` over stdio and wires it to the VS Code language client (`vscode-languageclient` 9.x). Picks up the server's semantic-token + inlay-hint + rename + code-action + signature-help providers automatically. |
| Snippets | 40+ snippets — `agent`, `protocol`, `tool`, `computer-use`, `swarm`, `eval-suite`, `cap`, `arena`, `budget`, `effect`, `sandbox`, `match`, `try`, `iflet`, plus declaration scaffolds (`struct`, `enum`, `trait`, `impl`) and supervisor patterns. |
| Palette commands | `Mighty: Run current file`, `Check current file`, `Build`, `Format`, `Inspect cost`, `Test --eval`, `Explain diagnostic`, `Restart Language Server`. |
| Status bar | A status-bar item shows today's LLM spend ($X.XX), refreshed every 30s from `~/.mty/observations.sqlite` via `mty inspect --cost --json`. Click to open the full breakdown. |
| Keybindings | `Ctrl+F5` (run current file) and `Ctrl+Shift+B` (check current file) when a Mighty editor is focused. |

## Install (from VSIX)

```
cd tools/vscode
npm install
npm run package
code --install-extension mighty-language-0.31.0.vsix
```

The extension targets VS Code ≥ 1.85. The `mty` binary must be on `PATH`
(or set `mighty.server.path` in your settings) — the LSP server, every
palette command, and the cost status bar all shell out to it.

## Develop from source

```
cd tools/vscode
npm install
npm run compile
```

Open this folder in VS Code and press **F5**. That launches an
**Extension Development Host** with the extension loaded; opening any
`.mty` file inside it will activate syntax highlighting and start the
LSP. Logs land in the **Mighty Language Server** output channel.

Watch-mode:

```
npm run watch
```

## Commands in detail

| Command | What it does |
| --- | --- |
| `Mighty: Run current file` | Saves the active editor, then runs `mty run <path>` in the integrated terminal. |
| `Mighty: Check current file` | Saves the editor and runs `mty check <path>` — useful for one-shot summaries; inline diagnostics still come from the LSP. |
| `Mighty: Build` | Runs `mty build` in the workspace terminal. |
| `Mighty: Format` | Saves the editor and runs `mty fmt <path>` (workspace-wide if no editor is open). |
| `Mighty: Inspect cost` | Opens `mty inspect --cost --since 24h --by provider` — full table view of LLM spend. |
| `Mighty: Test --eval` | Runs `mty test --eval` (with `--replay-only` by default — toggle via `mighty.test.replayOnly`). |
| `Mighty: Explain diagnostic` | Prompts for an `MTxxxx` code, runs `mty explain <code>`, and renders the result in a webview. |
| `Mighty: Restart Language Server` | Stops + restarts the LSP — useful after editing `mighty.server.path`. |

## Configuration

| Setting | Default | Description |
| --- | --- | --- |
| `mighty.server.path` | `mty` | Path to the `mty` binary. The extension spawns `<path> lsp` for the LSP. |
| `mighty.trace.server` | `off` | LSP trace verbosity. |
| `mighty.inlayHints.enable` | `false` | Show inferred-type hints next to `let` bindings and fn params. |
| `mighty.semanticTokens.enable` | `true` | Use the LSP's semantic-token classifier. |
| `mighty.costStatusBar.enable` | `true` | Toggle the status-bar item. |
| `mighty.costStatusBar.refreshSeconds` | `30` | Refresh interval (min 5s). |
| `mighty.test.replayOnly` | `true` | Default `Mighty: Test --eval` to `--replay-only`. |

## Status bar — what it shows

`graph-line Mighty: $X.XX (today)` — total spend over the last 24h
across every provider, computed from `~/.mty/observations.sqlite`
(the database every `std.llm` / `std.swarm` call writes to via the
v0.30 observation pipeline). When the DB is missing or empty the
item shows `$0.00`. Clicking it opens the full `mty inspect --cost`
table in a new terminal.

> Screenshots: `![cost-status](docs/cost-status.png)` and
> `![palette-commands](docs/palette.png)` will be added in a follow-up
> pass once the v0.31 release branch settles.

## Layout

```
tools/vscode/
├── README.md                        ← you are here
├── package.json                     ← extension manifest (publisher: hassard0)
├── tsconfig.json
├── .vscodeignore                    ← excludes src/, ts sources from the .vsix
├── src/
│   ├── extension.ts                 ← activation + LSP wiring
│   ├── commands.ts                  ← palette command handlers
│   └── status.ts                    ← cost status-bar item
├── syntaxes/
│   └── mighty.tmLanguage.json       ← TextMate grammar
├── snippets/
│   └── mighty.json                  ← 40+ snippets
├── language-configuration.json      ← brackets, comments, auto-close
└── icons/
    ├── mighty.svg
    └── mighty.png                   ← 128x128 package icon
```

## v0.32 follow-ups

- **DAP debug adapter** (`mty dap`) — placeholder commands in the
  manifest, but the real adapter ships next milestone.
- **Tree-sitter highlights** — once Track 1 lands a tree-sitter
  grammar, swap the TextMate grammar for it (or layer it on top via
  the `semantic-token` channel).
- **Inline `mty inspect --cost` panel** — a side-bar webview that
  graphs the cost-by-provider series instead of opening a terminal.
- **Per-file cost overlay** — annotate `@tool` / `swarm(...)` call
  sites with their cumulative spend in CodeLens.
- **Trace replay UI** — open a `.mty-trace` file and step through
  observations alongside the source.

## License

MIT — matches Mighty itself.
