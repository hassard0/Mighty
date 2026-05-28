// v0.32 Track A — VS Code DAP integration for Mighty.
//
// Registers `mty dap` as a debug adapter. The DAP server runs as a
// child process over stdio; VS Code's built-in debug UI handles the
// rest (breakpoint gutter, step controls, variables view).
//
// The launch.json contract this honours:
//
//   {
//     "type": "mighty",
//     "request": "launch",
//     "name": "Debug current file",
//     "program": "${file}",
//     "stopOnEntry": false,
//     "replayTrace": "${workspaceFolder}/trace.bin",  // optional
//     "recordTrace": "${workspaceFolder}/trace.bin"   // optional
//   }
//
// Users can also pin `"args": ["alpha", "beta"]` which the adapter
// forwards via `std.env.args()`.

import * as vscode from "vscode";

/** Factory that builds an executable descriptor pointing at `mty dap`. */
class MightyDebugAdapterDescriptorFactory
  implements vscode.DebugAdapterDescriptorFactory
{
  createDebugAdapterDescriptor(
    _session: vscode.DebugSession,
    _executable: vscode.DebugAdapterExecutable | undefined,
  ): vscode.ProviderResult<vscode.DebugAdapterDescriptor> {
    const command = (
      vscode.workspace
        .getConfiguration("mighty")
        .get<string>("server.path") || "mty"
    ).trim();
    // Pass through env, dropping the `undefined` slots that VS Code's
    // strict map type rejects.
    const env: { [k: string]: string } = {};
    for (const [k, v] of Object.entries(process.env)) {
      if (typeof v === "string") {
        env[k] = v;
      }
    }
    return new vscode.DebugAdapterExecutable(command, ["dap"], { env });
  }
}

/** Auto-fill missing fields when the user launches via F5 on a Mighty file. */
class MightyDebugConfigurationProvider
  implements vscode.DebugConfigurationProvider
{
  resolveDebugConfiguration(
    _folder: vscode.WorkspaceFolder | undefined,
    config: vscode.DebugConfiguration,
    _token?: vscode.CancellationToken,
  ): vscode.ProviderResult<vscode.DebugConfiguration> {
    // No config from launch.json — synthesise a default that runs the
    // active .mty file.
    if (!config.type && !config.request && !config.name) {
      const editor = vscode.window.activeTextEditor;
      if (editor && editor.document.languageId === "mighty") {
        config.type = "mighty";
        config.name = "Mighty: Debug current file";
        config.request = "launch";
        config.program = editor.document.uri.fsPath;
        config.stopOnEntry = false;
      }
    }
    if (!config.program) {
      return vscode.window
        .showInformationMessage(
          "Mighty: cannot debug — no `program` set in launch.json and no active .mty file.",
        )
        .then(() => undefined);
    }
    return config;
  }
}

/** Public entry point — call once during extension activation. */
export function registerMightyDap(context: vscode.ExtensionContext): void {
  const factory = new MightyDebugAdapterDescriptorFactory();
  const provider = new MightyDebugConfigurationProvider();

  context.subscriptions.push(
    vscode.debug.registerDebugAdapterDescriptorFactory("mighty", factory),
    vscode.debug.registerDebugConfigurationProvider("mighty", provider),
  );

  // Convenience command: F5-equivalent without a launch.json.
  context.subscriptions.push(
    vscode.commands.registerCommand("mighty.debugCurrentFile", async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.document.languageId !== "mighty") {
        vscode.window.showWarningMessage(
          "Mighty: open a .mty file first.",
        );
        return;
      }
      await vscode.debug.startDebugging(
        vscode.workspace.workspaceFolders?.[0],
        {
          type: "mighty",
          request: "launch",
          name: "Mighty: Debug current file",
          program: editor.document.uri.fsPath,
          stopOnEntry: false,
        },
      );
    }),
  );
}
