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
| Palette commands | `Mighty: Run current file`, `Check current file`, `Build`, `Format`, `Inspect cost` (now a webview), `Inspect cost (terminal)`, `Test --eval`, `Explain diagnostic`, `Restart Language Server`, `Debug current file`. |
| Cost status bar | A status-bar item shows today's LLM spend ($X.XX), refreshed every 30s from `~/.mty/observations.sqlite` via `mty inspect --cost --json`. Click to open the cost webview. |
| **Cost CodeLens** (v0.32) | Every `@tool(`, `swarm(`, `Member.<vendor>(`, and `.ask(` line gets a CodeLens with today's per-file cost + call count. Click to open the per-file breakdown in a terminal. |
| **Cost side panel** (v0.32) | `Mighty: Inspect cost` opens a theme-aware webview with summary cards (today / 7d / 30d / all-time), per-provider + per-model bar breakdowns, and a top-10 most expensive calls table. Auto-refreshes every 30s. |
| **Tree-sitter semantic tokens** (v0.32) | Stub provider registered with a forward-compatible token legend (incl. our custom `taintedType` token). The full grammar integration ships in v0.33 — see "Tree-sitter highlights" below for the theme-tweak that lets you start using `taintedType` today. |
| **Debugger** (v0.32) | Native DAP integration via `mty dap`. Hit `F5` on any `.mty` file — VS Code's built-in debug UI handles breakpoints, step-in / step-over / step-out, the variables view (showing each `let` binding + Track-F structured `tool_uses` for LLM calls), and the call stack. |
| Keybindings | `Ctrl+F5` (run current file), `Ctrl+Shift+B` (check current file), and `F5` (debug current file) when a Mighty editor is focused. |

## Debugging (v0.32)

Press `F5` on any open `.mty` file — the extension synthesises a default
launch config and shells out to `mty dap` over stdio. You don't have to
write a launch.json to get started.

To customise (e.g. to walk a recorded trace) drop a `.vscode/launch.json`
with:

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "mighty",
      "request": "launch",
      "name": "Mighty: Debug current file",
      "program": "${file}",
      "stopOnEntry": false,
      "args": [],
      "replayTrace": "${workspaceFolder}/trace.bin",
      "recordTrace": "${workspaceFolder}/trace.bin"
    }
  ]
}
```

`replayTrace` and `recordTrace` are both optional. Setting `recordTrace`
flips on `MTY_RECORD_TRACE` for the launched process — every event the
runtime emits gets appended to the file as the program runs. Setting
`replayTrace` drives the program through `ReplayDriver` instead of
executing live (the v0.32 Track F deliverable).

## Install (from VSIX)

```
cd tools/vscode
npm install
npm run package
code --install-extension mighty-language-0.32.0.vsix
```

The extension targets VS Code ≥ 1.85. The `mty` binary must be on `PATH`
(or set `mighty.server.path` in your settings) — the LSP server, every
palette command, the cost status bar, the CodeLens provider, and the
cost webview all shell out to it.

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
| `Mighty: Inspect cost` | Opens the cost side-panel webview (summary cards + breakdown bars + top-10 table). Auto-refreshes every 30s. |
| `Mighty: Inspect cost (terminal)` | Opens `mty inspect --cost --since 24h --by provider` in an integrated terminal — useful when you want to pipe the output. |
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
| `mighty.costCodeLens.enable` | `true` | Toggle the v0.32 cost CodeLens above call sites. |
| `mighty.test.replayOnly` | `true` | Default `Mighty: Test --eval` to `--replay-only`. |

## Cost CodeLens — what it shows

For every line in a Mighty file containing one of the call-site patterns
the extension recognises:

- `@tool(`
- `swarm(`
- `Member.anthropic(` / `Member.openai(` / `Member.gemini(` / `Member.bedrock(`
- `.ask(`

…we render a CodeLens above the line:

```
$0.04 total · 12 calls · last 24h
```

…or, if no observations have been recorded yet for the file:

```
$0.00 · no calls recorded
```

Clicking the lens opens `mty inspect --cost --top 10 --by agent <file>`
in a terminal — i.e. the same data, scoped to the source file.

The CodeLens provider polls SQLite every 60s and additionally
invalidates whenever you save the file. Turn it off with
`"mighty.costCodeLens.enable": false`.

> Screenshots: `![codelens](docs/codelens.png)` and
> `![cost-panel](docs/cost-panel.png)` will land in a follow-up commit
> once the v0.32 release branch settles.

## Cost side panel

`Mighty: Inspect cost` now opens a side-by-side webview rather than a
terminal. The panel has three sections:

1. **Summary cards** — today, last 7d, last 30d, and all-time spend.
2. **Breakdown bars** — per-provider and per-model spend rendered as
   plain HTML bars (no Chart.js dependency).
3. **Top-10 most expensive calls** — a sortable-on-rerender table
   showing timestamp / provider / model / agent / cost / latency.

The panel re-renders every 30s by shelling out to
`mty inspect --cost --json`. All colours use VS Code theme variables,
so it reflects whichever Light / Dark / High-Contrast theme you have
active.

The previous terminal-flavour command is still available as
**Mighty: Inspect cost (terminal)**.

## Tree-sitter highlights (v0.32 stub → v0.33)

v0.31 Track 1 shipped a tree-sitter grammar for Mighty at
`tools/tree-sitter/`. In v0.32 we register a **stub** semantic-token
provider that publishes a forward-compatible token legend. The full
grammar integration (WASM binding + parser + tree walk) ships in
v0.33; until then the TextMate grammar continues to handle highlighting.

Why a stub? Two reasons:

- It lets theme authors target our custom token vocabulary today, so
  their themes are ready when v0.33 lands.
- It pins the activation surface — when the WASM binding is wired in
  v0.33, the only code change required is filling in the
  `provideDocumentSemanticTokens` body in `src/tree-sitter.ts`.

### Custom token types

We expose one custom semantic-token type, `taintedType`, used for
`Tainted[T]` references. Themes can colour it via
`editor.tokenColorCustomizations` — drop this into your `settings.json`
to turn taint markers an attention-grabbing colour:

```jsonc
{
  "editor.tokenColorCustomizations": {
    "[Default Dark+]": {
      "textMateRules": [],
      "semanticHighlighting": true,
      "rules": {
        "taintedType:mighty": {
          "foreground": "#ff8a65",
          "fontStyle": "italic"
        }
      }
    }
  }
}
```

We also reserve three custom modifiers — `soft` (soft keywords like
`budget` / `swarm` / `agent`), `tainted` (anything carrying a `Tainted`
type), and `capability` (capability tokens like `!{net.http, fs.read}`).

## Status bar — what it shows

`graph-line Mighty: $X.XX (today)` — total spend over the last 24h
across every provider, computed from `~/.mty/observations.sqlite`
(the database every `std.llm` / `std.swarm` call writes to via the
v0.30 observation pipeline). When the DB is missing or empty the
item shows `$0.00`. Clicking it now opens the cost webview (see
above) — power users who want the raw terminal output can rebind
the status-bar `command` to `mighty.inspectCostTerminal` instead.

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
│   ├── status.ts                    ← cost status-bar item
│   ├── codelens.ts                  ← v0.32 cost CodeLens + snapshot cache
│   ├── tree-sitter.ts               ← v0.32 semantic-tokens stub (v0.33 plan)
│   └── webview/
│       └── costPanel.ts             ← v0.32 cost side-panel webview
├── syntaxes/
│   └── mighty.tmLanguage.json       ← TextMate grammar
├── snippets/
│   └── mighty.json                  ← 40+ snippets
├── language-configuration.json      ← brackets, comments, auto-close
└── icons/
    ├── mighty.svg
    └── mighty.png                   ← 128x128 package icon
```

## v0.33 follow-ups

- **Tree-sitter highlights (finish)** — ship the WASM grammar artifact
  and fill in `provideDocumentSemanticTokens` per the checklist at the
  bottom of `src/tree-sitter.ts`.
- **Per-span CodeLens granularity** — the v0.32 CodeLens shows the
  file-level total at every call site; once `mty inspect` exposes a
  `--by span` flag we'll wire per-line accuracy.
- **DAP debug adapter** (`mty dap`) — placeholder commands in the
  manifest, but the real adapter ships next milestone.
- **Trace replay UI** — open a `.mty-trace` file and step through
  observations alongside the source.
- **Webview interactivity** — once we ship the JS bundle for the cost
  panel, allow toggling time-window + drilling into a provider/model
  bar to see its top-10 contributing calls without a separate
  inspector.

## License

MIT — matches Mighty itself.
