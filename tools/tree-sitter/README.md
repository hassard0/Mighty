# tree-sitter-mighty

Tree-sitter grammar for the Mighty agent-first programming language.

This grammar powers syntax highlighting and structural editing for
Mighty source files (`.mty`) in every editor that consumes tree-sitter
grammars: Neovim, Helix, Zed, Emacs (via `tree-sitter-langs`), and
GitHub Linguist. The Mighty VS Code and JetBrains plugins
(`tools/vscode-extension/` and `tools/jetbrains-plugin/`) also consume
the query files in `queries/` directly.

The canonical parser for diagnostics + compilation lives in
`crates/mty-syntax/`. This tree-sitter grammar exists strictly for the
editor experience and is intentionally permissive — it tries to keep
producing a tree even on input the canonical parser would reject, so
half-typed buffers still highlight.

Status: v0.31 cut, tracking Mighty language v0.30 surface syntax.

## Layout

```
tools/tree-sitter/
├── grammar.js          ← the grammar
├── package.json        ← npm-publishable shape (publish as tree-sitter-mighty)
├── queries/
│   ├── highlights.scm  ← syntax highlighting captures
│   ├── locals.scm      ← scope tracking (go-to-def, rename)
│   ├── indents.scm     ← auto-indent rules
│   ├── injections.scm  ← embedded languages (html"…", format!, sql!, re!)
│   └── tags.scm        ← symbol extraction (linguist + outline panes)
├── corpus/             ← tree-sitter test fixtures
│   ├── basics.txt
│   ├── agents.txt
│   ├── llm_stdlib.txt
│   ├── taint.txt
│   └── computer_use.txt
└── README.md           ← you are here
```

## Build + test

Prerequisites:

```bash
npm install -g tree-sitter-cli
# or, per-project:
cd tools/tree-sitter && npm install
```

Then:

```bash
cd tools/tree-sitter
tree-sitter generate       # regenerate src/parser.c from grammar.js
tree-sitter build          # build the native parser
tree-sitter test           # run the corpus fixtures
tree-sitter parse ../../examples/01_hello.mty
tree-sitter parse ../../examples/*.mty   # sweep all 36 examples
```

A green `tree-sitter test` plus a clean parse over `examples/*.mty` is
the v0.31 acceptance bar.

## Install into your editor

### Neovim (via nvim-treesitter)

Add to your config:

```lua
require('nvim-treesitter.parsers').get_parser_configs().mighty = {
  install_info = {
    url = 'https://github.com/hassard0/Mighty',
    files = { 'tools/tree-sitter/src/parser.c' },
    branch = 'main',
    generate_requires_npm = false,
    requires_generate_from_grammar = true,
  },
  filetype = 'mighty',
}
vim.filetype.add({ extension = { mty = 'mighty' } })
```

Then `:TSInstall mighty` and copy the `queries/` directory into your
runtimepath as `queries/mighty/`. (Once tree-sitter-mighty ships to npm
+ the nvim-treesitter registry merges, this collapses to a single
`:TSInstall mighty`.)

### Helix

1. Build the grammar:
   ```bash
   cd tools/tree-sitter
   tree-sitter generate
   tree-sitter build
   cp build/mighty.* ~/.config/helix/runtime/grammars/
   ```
2. Copy queries:
   ```bash
   mkdir -p ~/.config/helix/runtime/queries/mighty
   cp queries/*.scm ~/.config/helix/runtime/queries/mighty/
   ```
3. Register the language in `~/.config/helix/languages.toml`:
   ```toml
   [[language]]
   name = "mighty"
   scope = "source.mighty"
   file-types = ["mty"]
   roots = ["mighty.toml"]
   comment-token = "//"
   indent = { tab-width = 2, unit = "  " }

   [[grammar]]
   name = "mighty"
   source = { git = "https://github.com/hassard0/Mighty", subpath = "tools/tree-sitter" }
   ```

### Zed

Drop the grammar in a Zed extension or, for local development:

```bash
cd tools/tree-sitter
tree-sitter generate
```

Then in your Zed extension's `extension.toml`:

```toml
[grammars.mighty]
repository = "https://github.com/hassard0/Mighty"
commit = "<sha>"
path = "tools/tree-sitter"
```

Zed automatically uses `queries/highlights.scm`, `queries/locals.scm`,
`queries/indents.scm`, and `queries/injections.scm` from the grammar
directory.

### Emacs (tree-sitter-langs)

Add an entry to `tree-sitter-langs--repos` pointing at this directory;
the rest is automatic.

## VS Code + JetBrains plugin tracks

The Mighty IDE plugins consume the same `queries/highlights.scm` and
`queries/tags.scm` files as Helix and Neovim:

- VS Code: see `tools/vscode-extension/README.md` (v0.31 Track 2).
- JetBrains: see `tools/jetbrains-plugin/README.md` (v0.31 Track 3).

Both tracks bundle a pre-built `parser.c` so end users don't need a
local tree-sitter toolchain.

## GitHub linguist

Once published to npm as `tree-sitter-mighty`, the linguist project
will accept a PR adding `.mty` as a recognised extension. The grammar +
tags queries in this directory are the artefacts linguist consumes.

## v0.32 follow-ups

This is the v0.31 cut. Known gaps to address in v0.32:

- `injections.scm` — `// LANG: <name>` hint comments preceding a string
  literal (the spec lists this surface; the tree-sitter capture is
  trickier than the static `format!` / `sql!` shapes covered here).
- `locals.scm` — broader scoping for protocol methods + supervisor
  children. The current set is enough for go-to-def on `let`-bindings
  and fn params; agent state field references need extra work.
- `tags.scm` — `impl Foo for Bar` blocks currently produce one
  `@definition.implementation` per block; splitting per-method tags
  would feed JetBrains' structure view better.
- Format-string interpolation: the current grammar treats `{}` runs as
  raw segments. A proper sub-grammar for `{name}`, `{name:format-spec}`
  would let editors highlight bindings inside the string.
- `Tainted[T]` highlight: highlighted via `@type.builtin.tainted` so
  themes can show it distinctly, but most themes won't define that
  capture yet. Coordinate with theme authors.
- Match-arm body greediness: corpus tests exercise the common cases;
  if an arm body extends across newlines without a comma, the parser
  may consume the next arm's pattern as part of the previous body. The
  v0.32 fix is an external scanner that emits a virtual newline-as-
  separator token; in the meantime, commas (which the formatter
  inserts) keep things tidy.
- Macro syntax model: `name!(...)`, `name!{...}`, `name![...]` are
  modeled via three immediate tokens (`!(`, `!{`, `![`). If Mighty
  ever ships a fourth tree-tree opener (e.g. `<<` for HTML literals
  inside macros), add a fourth immediate token alongside.

## Versioning

This package follows Mighty's `vMAJOR.MINOR.PATCH` cadence. The
`package.json` `version` field mirrors the Mighty workspace version so
`npm install tree-sitter-mighty@0.31.0` lines up with a Mighty release.

## Licence

Dual-licensed under MIT and Apache-2.0, matching the rest of the Mighty
workspace.
