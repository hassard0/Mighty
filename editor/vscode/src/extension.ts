// Stardust VS Code extension: spawns `sdust lsp` over stdio and wires
// it up via vscode-languageclient.
//
// Build:  npm install && npm run compile
// Package: npx vsce package  → produces stardust-0.5.0.vsix
// Install: code --install-extension stardust-0.5.0.vsix
//
// v0.5 features: semantic tokens, rename, inlay hints, code actions,
// signature help (in addition to the v0.2 baseline of diagnostics,
// hover, definition, formatting, completion).

import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: vscode.ExtensionContext): void {
  const config = vscode.workspace.getConfiguration("stardust");
  const command = (config.get<string>("server.path") || "sdust").trim();

  // `sdust lsp` speaks LSP 3.17 over stdio.
  const serverOptions: ServerOptions = {
    run: { command, args: ["lsp"], transport: TransportKind.stdio },
    debug: { command, args: ["lsp"], transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "stardust" },
      { scheme: "untitled", language: "stardust" },
    ],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.sd"),
      // Forward stardust.* settings changes so the server sees them.
      configurationSection: "stardust",
    },
    // The server declares semantic-token and inlay-hint providers; the
    // language client picks those up automatically.
  };

  client = new LanguageClient(
    "stardust",
    "Stardust Language Server",
    serverOptions,
    clientOptions,
  );

  // Expose an explicit "restart" command for users who change the
  // server.path setting or recover from a server crash.
  context.subscriptions.push(
    vscode.commands.registerCommand("stardust.restartServer", async () => {
      if (!client) return;
      await client.stop();
      await client.start();
      vscode.window.showInformationMessage("Stardust language server restarted.");
    }),
  );

  context.subscriptions.push({
    dispose: () => {
      client?.stop();
    },
  });

  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
