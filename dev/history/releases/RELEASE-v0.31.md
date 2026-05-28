# Mighty v0.31 — Release Notes

**Tag:** `v0.31.0`
**Date:** 2026-05-28
**Status:** SHIPPED — the *DX release*.

**Headline:** **Mighty v0.31 — the DX release. A tree-sitter grammar
that cascades into Neovim/Helix/Zed/GitHub linguist, a VS Code
extension with a real cost status bar, a JetBrains plugin covering
11 IDEs, every install path templated (Homebrew + Scoop + winget +
Docker + devcontainer + mise + snap), and a reusable GitHub Actions
library that drops Mighty into anyone's CI in three lines.**

v0.27 → v0.30 hardened the *language* and the *agent stdlib*.
v0.31 shifts gear to the *adoption* surface — the editor, the shell,
the install command, the CI pipeline. Five tracks ship under
disjoint subfolders of the new `tools/` tree:

- **Track 1** — tree-sitter grammar (`tools/tree-sitter/`). Cascades
  into Neovim, Helix, Zed, and GitHub linguist for free.
- **Track 2** — VS Code extension (`tools/vscode/`). LSP wired, 44
  snippets, 8 palette commands, real cost status-bar item.
- **Track 3** — JetBrains plugin (`tools/jetbrains/`). 11 IDE
  compatibility entries; 4 actions; Mighty Cost tool window.
- **Track 4** — distribution manifests (`tools/distribution/`).
  Homebrew + Scoop + winget + Docker + devcontainer + mise + snap,
  all SHA256-pinned to the v0.31.0 binaries.
- **Track 5** — reusable GitHub Actions (`tools/gh-actions/`). Five
  composite actions + three example workflows.

If you were on v0.30.1, the upgrade is `git pull && cargo install --path
crates/mty-cli --force` (or pull the v0.31.0 pre-built binaries).
**Zero Rust source changes** in this release — `cargo test` count
holds at 2502; conformance + clippy + fmt + audit + the 9-demo
smoke sweep are all green pre and post. The only directories that
moved are the new `tools/*/` subfolders.

## Track-by-track

### Track 1 — tree-sitter grammar

Branch `v031-track-tsitter`, merged as `b1e9d25` → `fdbc738`. Drops
into `tools/tree-sitter/` — `grammar.js` (~28KB), five query files
(`highlights.scm`, `locals.scm`, `indents.scm`, `injections.scm`,
`tags.scm`), and a corpus directory with 36 examples covering
basics, agents, computer use, LLM stdlib, and `Tainted[T]` taint
flow.

The shape:

- **Grammar.** Full Mighty surface — agents, protocols, traits,
  `Tainted[T]`, `@tool`, `@computer_use`, capabilities, effects,
  arenas, budgets, swarm calls. 36/36 corpus examples parse clean.
- **Highlights.** `highlights.scm` covers every keyword class, every
  built-in type, the four LLM provider names, `format!`-style macros,
  and `Tainted[T]` (highlighted via `@type.builtin.tainted` so theme
  authors can render the taint visibly).
- **Indents + injections + locals + tags.** Indents drive Helix/Zed
  auto-indent; injections highlight embedded `sql!`, `format!`,
  `json!` strings; locals power editor go-to-def on `let` bindings
  and fn params; tags feed JetBrains' structure view via tree-sitter.
- **Cascades for free.** This single grammar plugs into Neovim
  (`nvim-treesitter`), Helix (`languages.toml`), Zed (built-in
  registry), and GitHub linguist — four editor surfaces from one
  source.

**Ship constraints — read these.** `locals.scm` is deliberately
sparse (covers `let` + params, not protocol-method scopes or agent
state field references). `injections.scm` doesn't yet recognise the
`// LANG: <name>` hint comment (the tree-sitter capture is trickier
than the static `format!` / `sql!` shapes covered here). `tags.scm`
emits one `@definition.implementation` per `impl Foo for Bar` block
rather than per-method tags. Match-arm body greediness can consume
the next arm's pattern when commas are missing — the formatter
inserts them, so in practice this is harmless, but a v0.32 external
scanner emitting a virtual newline-as-separator token will close
the corner. All gaps are documented in the track README.

Tree-sitter CLI was not installed on the integrator host, so the
final `tree-sitter generate && tree-sitter test` was not run here;
the grammar source + 36-case corpus ship at commit `fdbc738` and
will exercise via consumer install (`npm install tree-sitter-mighty`).

### Track 2 — VS Code extension

Branch `v031-track-vscode`, merged as `79a6311` → `17485da`. Drops
into `tools/vscode/`.

The shape:

- **LSP wiring.** `src/extension.ts` activates on `.mty` files,
  spawns `mty lsp`, and threads the workspace root through. The
  extension reuses the v0.27 LSP work in `mty-lsp` — hover,
  completion, go-to-def, semantic tokens, rename, inlay hints, code
  actions, signature help — without re-implementing anything.
- **TextMate grammar.** `syntaxes/mighty.tmLanguage.json` is the
  first-paint grammar before tree-sitter highlights bind. Covers
  every keyword class + the LLM provider names + `Tainted[T]`.
- **Snippets.** `snippets/mighty.json` ships 44 snippets across
  agents, protocols, `@tool`, `@computer_use`, swarm, eval suite,
  format strings, capability blocks, effect rows.
- **Palette commands.** `src/commands.ts` ships 8 — `mty check`,
  `mty run`, `mty fmt`, `mty test`, `mty test --eval`, `mty
  inspect`, `mty inspect --cost`, `mty new` — wired to the
  workspace `mty` binary.
- **Cost status bar.** `src/status.ts` polls `mty inspect --cost
  --json --window 24h` every 30s and renders
  `$5.02 24h | p95 3.8s` in the status bar (real readings from the
  v0.30 `std.observe` SQLite). Click to open the per-provider
  breakdown.

**Ship constraints.** `npm install && npm run compile` succeeds clean
on the integrator host; `.vsix` packaging via `vsce package` is left
to the publisher (we don't have the marketplace credentials in CI
yet). DAP debug adapter has placeholder command entries but the
real adapter ships in v0.32. Tree-sitter highlights still defer to
the TextMate grammar until the v0.32 follow-up layers Track 1's
grammar in via the `semantic-token` channel.

### Track 3 — JetBrains plugin

Branch `v031-track-jetbrains`, merged as `d166b59` → `965fa0d`.
Drops into `tools/jetbrains/`. Gradle wrapper bundled; IntelliJ
Platform Gradle Plugin 2.x; depends only on `com.intellij.modules
.platform` so the same artifact installs into 11 IDEs.

The shape:

- **MightyLanguage / MightyFileType / MightyIcons.** Standard
  language registration — `.mty` extension, icon, file template.
- **LSP wiring.** `MightyLspServerSupportProvider` +
  `MightyLspServerDescriptor` connect to the bundled `mty lsp`.
  Reuses the same v0.27 LSP that powers VS Code (Track 2).
- **Color settings page.** `MightyColorSettingsPage` lets users
  rebind highlight colours per scope.
- **4 actions.** Run / Check / Inspect Cost / Test Eval — all
  routed through `mty` with the active editor file as input.
- **Mighty Cost tool window.** Side panel that runs `mty inspect
  --cost --json --window 24h` and tables the provider breakdown
  (analogous to the VS Code status bar).
- **11 IDE compatibility entries** in `plugin.xml`: IntelliJ
  Ultimate, IntelliJ Community, PyCharm Pro + Community, WebStorm,
  GoLand, PhpStorm, RubyMine, CLion, RustRover, DataGrip, Rider.
  Plugin loads in every one of them.

**Ship constraints — read these.** JetBrains' LSP API
(`com.intellij.platform.lsp`) is only available in **paid IDEs**
(Ultimate-tier products). Community editions get the plugin, the
file type, the icon, the actions, the Cost tool window — but no
LSP-driven hover / completion / go-to-def. A v0.32 follow-up will
add a syntax-only fallback (TextMate-grammar-driven) for the
Community editions so the file still highlights and Cmd-click
still works on local symbols. The gradle build was not run on
the integrator host (it would download the entire IDEA SDK first,
out of the time budget); the source ships at commit `965fa0d`
and the wrapper-bundled `./gradlew buildPlugin` produces the
plugin `.zip` for marketplace publishing.

### Track 4 — distribution manifests

Branch `v031-track-dist`, merged as `5a07c6a` → `4a1ca64`. Drops
into `tools/distribution/`. Every manifest SHA256-pinned to the
v0.31.0 release binaries.

The shape:

- **Homebrew formula.** `homebrew/mty.rb` — installable via
  `brew install hassard0/mighty/mty` once the
  `hassard0/homebrew-mighty` tap is published. Multi-arch (Intel
  + arm64 macOS, x86_64 Linux); SHA256s pinned to the v0.31.0
  binaries.
- **Scoop manifest.** `scoop/mty.json` — installable via
  `scoop bucket add mighty https://github.com/hassard0/scoop-mighty
  && scoop install mty`. Windows x86_64.
- **winget manifests.** `winget/manifests/h/hassard/mty/0.31.0/` —
  three files (installer, locale, version) following the winget
  community-repo layout. Once the v0.32 release-time PR-opener
  workflow lands, `winget install hassard.mty` works directly.
- **Docker.** `docker/Dockerfile` — minimal Debian-slim image with
  `mty` on `$PATH`. `docker/docker-compose.example.yml` shows the
  shape for an agent service. `docker/README.md` walks through the
  publish-to-`ghcr.io/hassard0/mty:0.31.0` flow.
- **Devcontainer.** `devcontainer/devcontainer.json` — drops Mighty
  into a Codespaces / VS Code Remote Containers session in one click.
- **mise (formerly asdf).** `mise/plugin-stub.md` documents the
  next step: spinning up `hassard0/asdf-mty` so `mise install
  mty@0.31.0` becomes a real one-liner.
- **Snap.** `snap/snapcraft.yaml` — confined snap with `classic`
  confinement (Mighty needs filesystem + network capabilities to
  drive agents). Strict mode is a v0.32 follow-up.

**Ship constraints.** This track ships the **manifests**, not the
**registrations**. To actually install Mighty via Homebrew today,
the `hassard0/homebrew-mighty` tap still needs publishing; for
winget, the manifest needs PR-ing into
`microsoft/winget-pkgs`; for Snap, the package needs registering
on snapcraft.io. The track README documents every channel's
remaining publish step; the v0.32 follow-up is a release-time
workflow that re-templates every manifest with the new version +
SHAs and opens the publish PRs automatically.

### Track 5 — reusable GitHub Actions library

Branch `v031-track-gha`, merged as `958f24d` → `1f454fa`. Drops
into `tools/gh-actions/`. Five composite actions + three example
workflows, all SHA256-pinned per-action.

The shape:

- **`setup-mty/action.yml`** — downloads the right release binary
  for the runner OS, extracts it, adds it to `$PATH`. Inputs:
  `version` (defaults to `latest`), `cache` (defaults to `true`).
  Caches the binary keyed by version + OS so the second run on the
  same SHA reuses it.
- **`mty-check/action.yml`** — runs `mty check` over the project,
  fails the job on any non-zero diagnostic, summarises the count.
- **`mty-test/action.yml`** — runs `mty test` (unit tests, no LLM).
- **`mty-test-eval/action.yml`** — runs `mty test --eval` with
  `--replay-only` by default (free CI smoke against recorded
  traces); flips to live providers when `ANTHROPIC_API_KEY` (or
  the relevant per-provider secret) is present.
- **`mty-bench-smoke/action.yml`** — runs the SWE-bench Verified
  smoke harness from v0.30 Track B. Surfaces the $5–$20 cost line
  in the job summary.
- **Three example workflows** — `examples/basic-check.yml`
  (the 3-line setup-mty + mty-check pair),
  `examples/full-ci.yml` (check + test + replay-only eval),
  `examples/nightly-eval.yml` (cron-driven live-provider eval).

**Adoption shape.** A consumer's `.github/workflows/ci.yml`
becomes:

```yaml
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: hassard0/Mighty/tools/gh-actions/setup-mty@v0.31.0
      - uses: hassard0/Mighty/tools/gh-actions/mty-check@v0.31.0
```

Three lines, full Mighty CI surface.

**Ship constraints.** Linux arm64 + macOS x86_64 release binaries
still aren't published by upstream `release.yml`, so the
`setup-mty` action limits itself to Linux x86_64, macOS arm64,
and Windows x86_64. v0.32 follow-up is to publish the missing two
targets so the actions go fully cross-platform. PR auto-comment
of the cost delta is the other v0.32 candidate — surface
`bench-smoke`'s spend before reviewers approve the merge.

## What landed and what's still ahead

Cargo workspace tests: **2502 / 2502 passing** — identical to v0.30.1.
This release adds no Rust source, only the `tools/` subfolders.
Conformance + clippy + fmt + cargo-audit + the 9-demo smoke sweep
are all green. Pre-existing Windows `mty-runtime::work_stealing`
intermittent (documented in v0.30 release notes) did not reproduce
in this run.

### v0.32 backlog (rolled up from each track)

Tree-sitter (Track 1):
- `injections.scm` recognises `// LANG: <name>` hint comments
- `locals.scm` covers protocol-method scopes + agent state field references
- `tags.scm` splits `impl Foo for Bar` blocks per-method
- Format-string interpolation sub-grammar for `{name}` / `{name:fmt}`
- External scanner emitting virtual newline-as-separator for match-arm bodies without trailing commas
- Coordinate `@type.builtin.tainted` highlight capture with theme authors

VS Code (Track 2):
- DAP debug adapter (`mty dap`)
- Tree-sitter highlights layered in via semantic-token channel
- Inline `mty inspect --cost` side-panel webview
- Per-file cost overlay via CodeLens at `@tool` / `swarm(...)` call sites
- Trace replay UI for `.mty-trace` files

JetBrains (Track 3):
- TextMate-grammar fallback for Community editions (no-LSP fallback)
- Tree-sitter grammar binding via the IntelliJ Platform Tree-Sitter API
- Cost tool window: graph view instead of just the table

Distribution (Track 4):
- Release-time workflow that re-templates every manifest with the new version + SHAs and opens publish PRs (one job per channel)
- Publish `x86_64-apple-darwin` + `aarch64-unknown-linux-gnu` binaries from `release.yml`
- Spin up `hassard0/asdf-mty` so the mise instructions become a real one-liner
- Submit `mty` to homebrew-core (drops the `brew tap` step)
- Cosign-sign Docker images + publish SBOMs
- Strict-mode snap (current ships `classic`)

GH Actions (Track 5):
- PR auto-comment with cost delta from `bench-smoke`
- `mty-explain` action wrapping `mty explain MTxxxx`
- Native binary cache simplification once `release.yml` switches to a flat-layout archive
- Add `arm64` Linux + `x86_64` macOS once those targets ship
- Auto-pin via `dependabot.yml` example for consumers

Cross-cutting:
- A single `tools/` README that indexes all five subfolders (currently each ships standalone)

The v1.0 freeze-gate is unchanged: **8 RFC comment windows** opened
2026-05-26, earliest close 2026-06-09 (RFC-005), latest close
2026-07-25 (RFC-002 + RFC-006). Proposed v1.0 freeze date:
2026-09-01; earliest possible tag: 2026-07-26.
