// Stardust VS Code extension: spawns `sdust lsp` over stdio and wires
// it up via vscode-languageclient.
//
// Build:  npm install && npm run compile
// Package: npx vsce package  → produces stardust-0.2.0.vsix
// Install: code --install-extension stardust-0.2.0.vsix

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
    },
  };

  client = new LanguageClient(
    "stardust",
    "Stardust Language Server",
    serverOptions,
    clientOptions,
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
