# Mighty — JetBrains IDE plugin

A single plugin that brings Mighty language support to the entire IntelliJ
Platform family: **IntelliJ IDEA, RustRover, PyCharm, WebStorm, GoLand,
PhpStorm, RubyMine, CLion, AppCode, DataGrip, Rider**.

The plugin is LSP-backed: it spawns `mty lsp` over stdio and lets the
IntelliJ Platform's LSP API handle diagnostics, hover, completion,
go-to-def, rename, formatting, inlay hints, and semantic highlighting.
On top of that it adds:

- `.mty` file-type association with a custom icon.
- A top-level **Mighty** menu and editor context-menu actions:
  - **Run Current Mighty File** (`mty run <file>`) — `Ctrl+Shift+M`
  - **Check Current Mighty File** (`mty check <file>`)
  - **Inspect Cost** (`mty inspect --cost`)
  - **Test Eval** (`mty test --eval`)
- A **Mighty Cost** tool window (bottom-right) that polls
  `mty inspect --cost --json` every 30 s.
- **New > Mighty File > Agent** and **Module** templates.
- A **Mighty** color settings page for token highlighting.
- **Settings > Tools > Mighty** to configure the `mty` binary path and
  cost-polling cadence.
- **Mighty Debug** run configuration type (v0.32): launches `mty dap`
  over stdio with a configured program path, optional replay/record
  trace, argv, and `stopOnEntry` toggle. Available in **Run → Edit
  Configurations…** under the "Mighty Debug" category. Works in both
  Community and Ultimate JetBrains IDEs (the debugger API is in
  platform core, unlike the LSP API).

## Debugging (v0.32)

Create a new run configuration via **Run → Edit Configurations… → +
→ Mighty Debug**, fill in the program path (an absolute `.mty` path),
then hit the standard Debug button. The configuration spawns `mty dap`
over stdio; per-line breakpoints come from the LSP gutter. The DAP
↔ XDebugger frontend that surfaces full step-in / step-over UI is
the v0.33 follow-up — what ships in v0.32 is the run-target plumbing,
console mode, and the option surfaces.

---

## Prerequisites

- JDK **17 or newer** (JDK 21 verified). The Gradle wrapper bundled in this
  directory pulls the right Gradle distribution automatically.
- The `mty` CLI on `$PATH`, or a configured absolute path in
  *Settings > Tools > Mighty*. The CLI must expose the LSP backbone via
  `mty lsp`.
- IntelliJ Platform IDE **2024.1 or newer** (build `241+`) to install the
  resulting plugin. JetBrains' LSP API is bundled in the **paid** IDEs
  (IDEA Ultimate, PyCharm Pro, WebStorm, GoLand, PhpStorm, RubyMine, CLion,
  RustRover, DataGrip, Rider). Community editions of IDEA / PyCharm don't
  ship the LSP API, so the plugin's editor features will be unavailable
  there even though the plugin itself loads.

## Build

```bash
cd tools/jetbrains
./gradlew buildPlugin
```

The Gradle task downloads the IntelliJ Platform Gradle Plugin 2.x and the
target IDE distribution (IntelliJ IDEA Community 2024.1 by default), then
produces:

```
build/distributions/Mighty-0.31.0.zip
```

That ZIP is the installable plugin artifact — ship it to the JetBrains
Marketplace or distribute it directly.

## Install from disk

1. Open any JetBrains IDE.
2. *Settings/Preferences* → *Plugins* → cog icon → **Install Plugin from
   Disk…**
3. Select `build/distributions/Mighty-0.31.0.zip`.
4. Restart the IDE when prompted.

## Run a sandbox IDE during development

```bash
./gradlew runIde
```

> **Heads-up.** The first invocation downloads ~500 MB of IntelliJ IDEA
> Community. The build script is wired up for this — the agent-driven
> bootstrap in `tools/jetbrains/` deliberately skips `runIde` to avoid the
> download.

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
pluginVersion  = 0.31.0
pluginSinceBuild = 232          # IntelliJ Platform 2023.2+
platformVersion  = 2024.1       # IDE built against
```

## Supported IDEs

The plugin depends only on `com.intellij.modules.platform`, which is the
common module shared by every JetBrains IDE. That makes the same artifact
installable in:

1. IntelliJ IDEA Ultimate (`idea`)
2. IntelliJ IDEA Community (`idea-community`)
3. PyCharm (Pro + Community)
4. WebStorm
5. GoLand
6. PhpStorm
7. RubyMine
8. CLion
9. RustRover
10. DataGrip
11. Rider

(AppCode is grandfathered into the same module set; it has been
discontinued by JetBrains but the plugin still loads on archived
installs.)

## Layout

```
tools/jetbrains/
├── build.gradle.kts                # IntelliJ Platform Gradle Plugin 2.x
├── settings.gradle.kts
├── gradle.properties               # plugin coords + IDE since-build
├── gradle/wrapper/
├── src/main/
│   ├── kotlin/dev/mighty/jetbrains/
│   │   ├── MightyLanguage.kt
│   │   ├── MightyFileType.kt
│   │   ├── MightyIcons.kt
│   │   ├── MightyLspServerSupportProvider.kt
│   │   ├── MightyLspServerDescriptor.kt
│   │   ├── MightyColorSettingsPage.kt
│   │   ├── actions/                # Run / Check / InspectCost / TestEval
│   │   ├── settings/               # Settings > Tools > Mighty
│   │   └── toolwindow/             # Mighty Cost tool window
│   └── resources/
│       ├── META-INF/plugin.xml     # 11 IDE compatibility entries
│       ├── icons/
│       └── fileTemplates/internal/
└── README.md
```

## License

MIT — see the top-level [`LICENSE`](../../LICENSE).
