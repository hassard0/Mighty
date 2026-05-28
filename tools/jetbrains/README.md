# Mighty — JetBrains IDE plugin

A single plugin that brings Mighty language support to the entire IntelliJ
Platform family: **IntelliJ IDEA (Community + Ultimate), RustRover, PyCharm
(Community + Pro), WebStorm, GoLand, PhpStorm, RubyMine, CLion, AppCode,
DataGrip, Rider**.

The plugin auto-adapts to the host IDE: paid editions get LSP-backed
editor features (completion / hover / go-to-def / inlay hints / semantic
tokens) via the bundled `mty lsp` server; **Community editions** get
TextMate-grammar-driven syntax highlighting plus every non-LSP feature
listed below.

## What works where

| Feature | Community (IDEA, PyCharm CE) | Ultimate / paid IDEs |
| --- | --- | --- |
| `.mty` file-type association + icon | yes | yes |
| **TextMate-grammar syntax highlighting** | **yes** (fallback) | yes (LSP semantic tokens preferred) |
| Run / Check / Inspect Cost / Test Eval actions | yes | yes |
| Cost dashboard tool window (TreeTable) | yes | yes |
| New > Mighty File templates (Agent + Module) | yes | yes |
| Mighty color settings page | yes | yes |
| Settings > Tools > Mighty | yes | yes |
| **Mighty Debug run configuration** (v0.32) | **yes** | **yes** |
| LSP completion / hover / go-to-def / rename | no | **yes** |
| LSP inlay hints / semantic tokens / formatting | no | **yes** |
| LSP diagnostics in the editor gutter | no | **yes** |

The LSP-only features depend on `com.intellij.platform.lsp.api`, a module
JetBrains only ships in their **paid** IDEs. The Mighty plugin still loads
on Community — it just doesn't activate the LSP wiring there. The LSP
extensions are pulled in via `<depends optional="true"
config-file="mighty-lsp.xml">com.intellij.modules.lsp</depends>`, so the
platform's plugin loader skips them cleanly when the module is absent.

The TextMate fallback uses the bundled `org.jetbrains.plugins.textmate`
plugin (also marked `optional` — the rare IDE that doesn't ship it still
gets the file-type registration, CLI actions, and tool window, just
without syntax highlighting).

The **Mighty Debug** run configuration (v0.32 Track A) lives in
platform core (`com.intellij.execution`) rather than the LSP module,
so it works in both Community and Ultimate IDEs.

Top-level **Mighty** menu shortcuts (also on the editor context menu):

- **Run Current Mighty File** (`mty run <file>`) — `Ctrl+Shift+M`
- **Check Current Mighty File** (`mty check <file>`)
- **Inspect Cost** (`mty inspect --cost`)
- **Test Eval** (`mty test --eval`)

## Debugging (v0.32)

Create a new run configuration via **Run → Edit Configurations… → +
→ Mighty Debug**, fill in the program path (an absolute `.mty` path),
then hit the standard Debug button. The configuration spawns `mty dap`
over stdio with an optional replay/record trace, argv, and `stopOnEntry`
toggle; per-line breakpoints come from the LSP gutter on Ultimate. The
DAP ↔ XDebugger frontend that surfaces full step-in / step-over UI is
the v0.33 follow-up — what ships in v0.32 is the run-target plumbing,
console mode, and the option surfaces.

---

## Prerequisites

- JDK **17 or newer** (JDK 21 verified). The Gradle wrapper bundled in this
  directory pulls the right Gradle distribution automatically.
- The `mty` CLI on `$PATH`, or a configured absolute path in
  *Settings > Tools > Mighty*. The CLI must expose the LSP backbone via
  `mty lsp` if you want the LSP-driven features on an Ultimate-class IDE.
- IntelliJ Platform IDE **2023.2 or newer** (build `232+`) to install the
  plugin. Both Community and Ultimate editions are supported as of v0.32.

## Build

```bash
cd tools/jetbrains
./gradlew buildPlugin
```

The Gradle task downloads the IntelliJ Platform Gradle Plugin 2.x and the
target IDE distribution (IntelliJ IDEA Ultimate 2024.1 by default — we
compile against Ultimate so the LSP symbols resolve, but the resulting
plugin runs on Community too), then produces:

```
build/distributions/mighty-0.32.0.zip
```

That ZIP is the installable plugin artifact — ship it to the JetBrains
Marketplace or distribute it directly.

> **Windows path-length tip.** The IntelliJ Platform plugin caches IDE
> distributions under `~/.gradle/caches/` which on Windows can blow past
> the 260-character path limit and leave undeletable directories. Point
> Gradle at a short cache: `GRADLE_USER_HOME=C:/gradle-cache ./gradlew
> buildPlugin`.

## Install from disk

1. Open any JetBrains IDE — Community or Ultimate.
2. *Settings/Preferences* → *Plugins* → cog icon → **Install Plugin from
   Disk…**
3. Select `build/distributions/mighty-0.32.0.zip`.
4. Restart the IDE when prompted.

On first start the plugin extracts its bundled TextMate grammar
(`resources/textmate/`) into `$IDE_SYSTEM/mighty/textmate-bundle/` and
asks `TextMateService` to re-scan. If you ever want to point at the
grammar manually, *Settings > Editor > TextMate Bundles* → **+** → select
that directory.

## Run a sandbox IDE during development

```bash
./gradlew runIde
```

> **Heads-up.** The first invocation downloads ~500 MB of IntelliJ IDEA.
> The build script targets Ultimate, but you can flip to Community for
> a smoke test by overriding `platformType=IC` in `gradle.properties`.

## Verify against multiple IDE versions

```bash
./gradlew verifyPlugin
```

Plugin Verifier checks the bytecode against every IDE the manifest claims
to support and flags API mismatches.

## Configuration

Edit `gradle.properties` to bump plugin coordinates:

```
pluginGroup    = dev.mighty.jetbrains
pluginVersion  = 0.32.0
pluginSinceBuild = 232          # IntelliJ Platform 2023.2+ (CE + Ultimate)
platformVersion  = 2024.1       # IDE built against (Ultimate by default)
platformType     = IU           # IU = Ultimate (LSP symbols resolve)
```

## Supported IDEs

The plugin depends only on `com.intellij.modules.platform` (with
`org.jetbrains.plugins.textmate` and `com.intellij.modules.lsp` marked
optional), which means the same artifact installs in:

1. IntelliJ IDEA Ultimate
2. IntelliJ IDEA Community  *(new in v0.32 — LSP features unavailable)*
3. PyCharm Pro
4. PyCharm Community  *(new in v0.32 — LSP features unavailable)*
5. WebStorm
6. GoLand
7. PhpStorm
8. RubyMine
9. CLion
10. RustRover
11. DataGrip
12. Rider

(AppCode is grandfathered into the same module set; it has been
discontinued by JetBrains but the plugin still loads on archived
installs.)

## Layout

```
tools/jetbrains/
├── build.gradle.kts                # IntelliJ Platform Gradle Plugin 2.x
├── settings.gradle.kts
├── gradle.properties               # plugin coords + IDE since-build (232+)
├── gradle/wrapper/
├── src/main/
│   ├── kotlin/dev/mighty/jetbrains/
│   │   ├── MightyLanguage.kt
│   │   ├── MightyFileType.kt
│   │   ├── MightyIcons.kt
│   │   ├── MightyLspServerSupportProvider.kt  # loaded only on Ultimate
│   │   ├── MightyLspServerDescriptor.kt       # loaded only on Ultimate
│   │   ├── MightyColorSettingsPage.kt
│   │   ├── actions/                # Run / Check / InspectCost / TestEval
│   │   ├── settings/               # Settings > Tools > Mighty
│   │   ├── textmate/               # TextMate fallback registrar (Community)
│   │   └── toolwindow/             # Mighty Cost tool window (TreeTable)
│   └── resources/
│       ├── META-INF/
│       │   ├── plugin.xml          # base manifest, since-build 232
│       │   ├── mighty-lsp.xml      # optional config-file, loads only with LSP module
│       │   └── mighty-textmate.xml # optional config-file, loads only with TextMate plugin
│       ├── icons/
│       ├── fileTemplates/internal/
│       └── textmate/               # bundled TextMate grammar
│           ├── package.json
│           ├── language-configuration.json
│           └── Syntaxes/
│               └── mighty.tmLanguage.json
└── README.md
```

## Grammar source-of-truth

The TextMate grammar at
`src/main/resources/textmate/Syntaxes/mighty.tmLanguage.json` is currently
a **copy** of `tools/vscode/syntaxes/mighty.tmLanguage.json`. JetBrains
plugin packaging doesn't follow symlinks, so two copies live in the tree.

**v0.33 chore:** extract a single canonical grammar (e.g.
`tools/grammars/mighty.tmLanguage.json`) and point both VS Code + JetBrains
builds at it via a build-time copy step. Until then, edit the VS Code
file first and mirror the change here.

## License

MIT — see the top-level [`LICENSE`](../../LICENSE).
