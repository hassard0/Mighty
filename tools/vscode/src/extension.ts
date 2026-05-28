// Mighty VS Code extension — v0.32 Track B.
//
// Wires the following together:
//
//   1. LSP client → `mty lsp` (stdio).
//   2. Palette commands → registered in commands.ts, each shells out to
//      `mty <subcommand>` in the integrated terminal (or runs as a task
//      where structured output matters).
//   3. Cost status-bar item → polls ~/.mty/observations.sqlite via
//      `mty inspect --cost --json` every N seconds (default 30s) and
//      surfaces today's spend in the bottom-right corner.
//   4. **NEW in v0.32** — Cost CodeLens provider: annotates every line
//      containing `@tool(`, `swarm(`, `Member.<vendor>(`, or `.ask(`
//      with the file's 24h cost + call-count. Polls every 60s plus on
//      document save.
//   5. **NEW in v0.32** — Cost side-panel webview: replaces the
//      terminal-based `Mighty: Inspect cost` with a theme-aware HTML
//      panel (summary cards + per-provider bars + top-10 table). The
//      old terminal flavour stays available as
//      `Mighty: Inspect cost (terminal)`.
//   6. **NEW in v0.32** — Tree-sitter semantic-tokens provider (stub).
//      Registers a placeholder provider so theme files can target our
//      forward-compatible token legend; full grammar integration lands
//      in v0.33.
//
// Build:    npm install && npm run compile
// Package:  npm run package  → produces mighty-language-0.32.0.vsix
// Install:  code --install-extension mighty-language-0.32.0.vsix
// Debug:    F5 from this folder spins up an Extension Development Host.

import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

import { registerCommands } from "./commands";
import {
  COST_LENS_COMMAND,
  CostSnapshotCache,
  MightyCostCodeLensProvider,
} from "./codelens";
// v0.32 Track A — `mty dap` over stdio. Registers a debug-adapter
// descriptor factory + a default config resolver so users can hit F5
// on any `.mty` file without writing a launch.json. See `dap.ts`.
import { registerMightyDap } from "./dap";
import { CostStatusBar } from "./status";
import { registerSemanticTokens } from "./tree-sitter";
import { CostPanelController } from "./webview/costPanel";

let client: LanguageClient | undefined;
let costBar: CostStatusBar | undefined;
let costPanel: CostPanelController | undefined;
let snapshotCache: CostSnapshotCache | undefined;

export async function activate(
  context: vscode.ExtensionContext,
): Promise<void> {
  const config = vscode.workspace.getConfiguration("mighty");
  const command = (config.get<string>("server.path") || "mty").trim();

  // `mty lsp` speaks LSP 3.17 over stdio.
  const serverOptions: ServerOptions = {
    run: { command, args: ["lsp"], transport: TransportKind.stdio },
    debug: { command, args: ["lsp"], transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "mighty" },
      { scheme: "untitled", language: "mighty" },
    ],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.mty"),
      configurationSection: "mighty",
    },
    initializationOptions: {
      inlayHints: config.get<boolean>("inlayHints.enable", false),
      semanticTokens: config.get<boolean>("semanticTokens.enable", true),
    },
  };

  client = new LanguageClient(
    "mighty",
    "Mighty Language Server",
    serverOptions,
    clientOptions,
  );

  // Restart command is wired here (rather than in commands.ts) because
  // it closes over the live client handle.
  context.subscriptions.push(
    vscode.commands.registerCommand("mighty.restartServer", async () => {
      if (!client) {
        vscode.window.showWarningMessage(
          "Mighty: language server is not running.",
        );
        return;
      }
      await client.stop();
      await client.start();
      vscode.window.showInformationMessage(
        "Mighty language server restarted.",
      );
    }),
  );

  // All palette + context commands live in their own module so this
  // file stays activation-focused.
  registerCommands(context);

  // v0.32 Track A — debugger integration. Idempotent + safe even if
  // the user never opens a launch.json (we synthesise a default).
  registerMightyDap(context);

  // Cost status bar — best-effort. If `mty` isn't on PATH or the
  // observations DB is missing, the bar shows `$0.00` rather than
  // surfacing an error.
  if (config.get<boolean>("costStatusBar.enable", true)) {
    costBar = new CostStatusBar(context, command);
    costBar.start();
  }

  // Cost snapshot cache (60s poll). Shared between the CodeLens
  // provider and any future per-file UI.
  snapshotCache = new CostSnapshotCache(command);
  snapshotCache.start();
  context.subscriptions.push(snapshotCache);

  // CodeLens provider — line-level cost annotations above every
  // recognised call site.
  if (config.get<boolean>("costCodeLens.enable", true)) {
    const lensProvider = new MightyCostCodeLensProvider(snapshotCache);
    context.subscriptions.push(
      vscode.languages.registerCodeLensProvider(
        [
          { scheme: "file", language: "mighty" },
          { scheme: "untitled", language: "mighty" },
        ],
        lensProvider,
      ),
      vscode.workspace.onDidSaveTextDocument((doc) => {
        if (doc.languageId !== "mighty") return;
        // Save = user just changed call sites; refresh the snapshot
        // and re-fire the CodeLens change event.
        void snapshotCache?.refresh();
        lensProvider.fireChanged();
      }),
      lensProvider,
    );
  }

  // Click handler for cost CodeLenses — opens the per-file cost view
  // in a terminal. Lives here because we want one shared handler
  // across every lens.
  context.subscriptions.push(
    vscode.commands.registerCommand(
      COST_LENS_COMMAND,
      (filePath?: string) => {
        const path = filePath ?? activeFsPath();
        if (!path) {
          vscode.window.showWarningMessage(
            "Mighty: no file path for cost lookup.",
          );
          return;
        }
        const mtyPath = (
          vscode.workspace
            .getConfiguration("mighty")
            .get<string>("server.path") || "mty"
        ).trim();
        const term = vscode.window.createTerminal({
          name: "Mighty Cost (file)",
        });
        term.show(true);
        term.sendText(
          `${mtyPath} inspect --cost --top 10 --by agent "${path.replace(/"/g, '\\"')}"`,
        );
      },
    ),
  );

  // Cost side-panel webview. The command-id `mighty.inspectCost` is
  // preserved for backwards compatibility — it now opens the webview;
  // power users who want the raw terminal output get
  // `mighty.inspectCostTerminal` instead (registered in commands.ts).
  costPanel = new CostPanelController(context, command);
  context.subscriptions.push(
    vscode.commands.registerCommand("mighty.inspectCost", () => {
      costPanel?.show();
    }),
    costPanel,
  );

  // Tree-sitter semantic-tokens provider (v0.32 stub — see
  // src/tree-sitter.ts for the v0.33 roadmap inside the file).
  registerSemanticTokens(context);

  // Re-evaluate the cost bar when the user toggles the setting.
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration("mighty.costStatusBar.enable")) {
        const enabled = vscode.workspace
          .getConfiguration("mighty")
          .get<boolean>("costStatusBar.enable", true);
        if (enabled && !costBar) {
          costBar = new CostStatusBar(context, command);
          costBar.start();
        } else if (!enabled && costBar) {
          costBar.dispose();
          costBar = undefined;
        }
      }
    }),
  );

  context.subscriptions.push({
    dispose: () => {
      client?.stop();
      costBar?.dispose();
      costPanel?.dispose();
      snapshotCache?.dispose();
    },
  });

  // Start the LSP. Failures bubble up via the standard "Mighty Language
  // Server" output channel; we don't want to block activation.
  try {
    await client.start();
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    vscode.window.showWarningMessage(
      `Mighty LSP failed to start (${msg}). Set mighty.server.path or install the mty binary.`,
    );
  }
}

function activeFsPath(): string | undefined {
  const editor = vscode.window.activeTextEditor;
  if (!editor) return undefined;
  if (editor.document.languageId !== "mighty") return undefined;
  return editor.document.uri.fsPath;
}

export function deactivate(): Thenable<void> | undefined {
  costBar?.dispose();
  costPanel?.dispose();
  snapshotCache?.dispose();
  return client?.stop();
}
