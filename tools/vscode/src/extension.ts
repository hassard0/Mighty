// Mighty VS Code extension — v0.31 Track 2.
//
// Wires three things up:
//
//   1. LSP client → `mty lsp` (stdio).
//   2. Palette commands → registered in commands.ts, each shells out to
//      `mty <subcommand>` in the integrated terminal (or runs as a task
//      where structured output matters).
//   3. Cost status-bar item → polls ~/.mty/observations.sqlite via
//      `mty inspect --cost --json` every N seconds (default 30s) and
//      surfaces today's spend in the bottom-right corner. Clicking it
//      opens a full `mty inspect --cost` table in a new terminal.
//
// Build:    npm install && npm run compile
// Package:  npm run package  → produces mighty-language-0.31.0.vsix
// Install:  code --install-extension mighty-language-0.31.0.vsix
// Debug:    F5 from this folder spins up an Extension Development Host.

import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

import { registerCommands } from "./commands";
// v0.32 Track A — `mty dap` over stdio. Registers a debug-adapter
// descriptor factory + a default config resolver so users can hit F5
// on any `.mty` file without writing a launch.json. See `dap.ts`.
import { registerMightyDap } from "./dap";
import { CostStatusBar } from "./status";

let client: LanguageClient | undefined;
let costBar: CostStatusBar | undefined;

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
    // The server declares semantic-token and inlay-hint providers; the
    // language client picks those up automatically.
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

export function deactivate(): Thenable<void> | undefined {
  costBar?.dispose();
  return client?.stop();
}
