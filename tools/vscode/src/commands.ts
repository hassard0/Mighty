// Palette commands for the Mighty extension.
//
// Each command shells out to the `mty` CLI in the integrated terminal
// (so users keep stdout, ANSI colours, and the same auth env as their
// shell). The terminal is reused per-command — running "Mighty: Build"
// twice doesn't open two panels.
//
// Explain-diagnostic is the exception: it captures `mty explain MTxxxx`
// stdout and renders it in a webview, because dropping a single help
// blob into a terminal is awkward.

import * as cp from "child_process";
import * as vscode from "vscode";

/** Cached integrated terminals, keyed by display name. */
const terminals = new Map<string, vscode.Terminal>();

function mtyBinary(): string {
  return (
    vscode.workspace.getConfiguration("mighty").get<string>("server.path") ||
    "mty"
  ).trim();
}

function quote(path: string): string {
  // Wrap in double-quotes; escape any embedded double-quote. Good
  // enough for every shell VS Code launches integrated terminals in.
  return `"${path.replace(/"/g, '\\"')}"`;
}

function getOrCreateTerminal(name: string): vscode.Terminal {
  const existing = terminals.get(name);
  if (existing && existing.exitStatus === undefined) {
    return existing;
  }
  const term = vscode.window.createTerminal({ name });
  terminals.set(name, term);
  return term;
}

function activeMtyFilePath(): string | undefined {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {
    vscode.window.showWarningMessage("Mighty: no active editor.");
    return undefined;
  }
  if (editor.document.languageId !== "mighty") {
    vscode.window.showWarningMessage(
      "Mighty: active file is not a Mighty (.mty) file.",
    );
    return undefined;
  }
  return editor.document.uri.fsPath;
}

function runInTerminal(name: string, cmd: string): void {
  const term = getOrCreateTerminal(name);
  term.show(true);
  term.sendText(cmd);
}

export function registerCommands(context: vscode.ExtensionContext): void {
  context.subscriptions.push(
    vscode.commands.registerCommand("mighty.run", async () => {
      const path = activeMtyFilePath();
      if (!path) return;
      await vscode.window.activeTextEditor?.document.save();
      runInTerminal("Mighty Run", `${mtyBinary()} run ${quote(path)}`);
    }),

    vscode.commands.registerCommand("mighty.check", async () => {
      const path = activeMtyFilePath();
      if (!path) return;
      await vscode.window.activeTextEditor?.document.save();
      // `mty check` exits 0/non-zero; the LSP already streams
      // diagnostics inline, but the terminal output gives users a
      // structured summary they can copy into a bug report.
      runInTerminal("Mighty Check", `${mtyBinary()} check ${quote(path)}`);
    }),

    vscode.commands.registerCommand("mighty.build", () => {
      runInTerminal("Mighty Build", `${mtyBinary()} build`);
    }),

    vscode.commands.registerCommand("mighty.fmt", async () => {
      const path = activeMtyFilePath();
      if (!path) {
        // No active file — format the workspace.
        runInTerminal("Mighty Format", `${mtyBinary()} fmt`);
        return;
      }
      await vscode.window.activeTextEditor?.document.save();
      runInTerminal("Mighty Format", `${mtyBinary()} fmt ${quote(path)}`);
    }),

    // Power-user variant: dumps `mty inspect --cost` straight into an
    // integrated terminal. The "Mighty: Inspect cost" command (without
    // the `(terminal)` suffix) now opens the side-panel webview —
    // registered in extension.ts.
    vscode.commands.registerCommand("mighty.inspectCostTerminal", () => {
      runInTerminal(
        "Mighty Cost",
        `${mtyBinary()} inspect --cost --since 24h --by provider`,
      );
    }),

    vscode.commands.registerCommand("mighty.testEval", () => {
      const replayOnly = vscode.workspace
        .getConfiguration("mighty")
        .get<boolean>("test.replayOnly", true);
      const args = replayOnly ? "--eval --replay-only" : "--eval";
      runInTerminal("Mighty Test", `${mtyBinary()} test ${args}`);
    }),

    vscode.commands.registerCommand("mighty.explainDiagnostic", async () => {
      const code = await vscode.window.showInputBox({
        prompt: "Diagnostic code to explain",
        placeHolder: "e.g. MT0042",
        validateInput: (v) =>
          /^MT\d{4}$/i.test(v.trim())
            ? undefined
            : "Expected an MTxxxx diagnostic code (e.g. MT0042).",
      });
      if (!code) return;
      const normalised = code.trim().toUpperCase();

      // Capture stdout rather than streaming to a terminal — webview
      // gives us markdown rendering + better readability.
      await renderExplainWebview(context, normalised);
    }),
  );
}

/**
 * Run `mty explain <code>` and render its stdout in a webview panel.
 * We treat stdout as plain text; if a future `mty explain` ships a
 * `--format markdown` flag, this is the place to wire it through.
 */
async function renderExplainWebview(
  context: vscode.ExtensionContext,
  code: string,
): Promise<void> {
  const panel = vscode.window.createWebviewPanel(
    "mighty.explain",
    `Mighty: ${code}`,
    vscode.ViewColumn.Beside,
    { enableScripts: false, retainContextWhenHidden: true },
  );

  panel.webview.html = renderHtml(
    code,
    "Running `mty explain` — one moment...",
  );

  try {
    const stdout = await runCapture(mtyBinary(), ["explain", code]);
    panel.webview.html = renderHtml(code, stdout);
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    panel.webview.html = renderHtml(
      code,
      `Failed to run \`mty explain ${code}\`:\n\n${msg}\n\n` +
        `Verify that the \`mty\` binary is on PATH or set the \`mighty.server.path\` setting.`,
    );
  }

  context.subscriptions.push(panel);
}

/** Spawn a process and resolve with stdout, rejecting on non-zero exit. */
function runCapture(cmd: string, args: string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = cp.spawn(cmd, args, { shell: false });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk: Buffer) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk: Buffer) => {
      stderr += chunk.toString();
    });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolve(stdout);
      } else {
        reject(
          new Error(
            `${cmd} ${args.join(" ")} exited with code ${code}\n${stderr}`,
          ),
        );
      }
    });
  });
}

/** Wrap CLI output in a minimal webview HTML shell. */
function renderHtml(code: string, body: string): string {
  const escaped = body
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
  return `<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Mighty: ${code}</title>
    <style>
      body {
        font-family: var(--vscode-editor-font-family, "Menlo", monospace);
        font-size: var(--vscode-editor-font-size, 13px);
        color: var(--vscode-editor-foreground);
        background: var(--vscode-editor-background);
        padding: 16px;
      }
      h1 {
        font-size: 1.4em;
        margin-top: 0;
        color: var(--vscode-textLink-foreground);
      }
      pre {
        white-space: pre-wrap;
        word-wrap: break-word;
        background: var(--vscode-textBlockQuote-background, transparent);
        border-left: 3px solid var(--vscode-textBlockQuote-border, #888);
        padding: 8px 12px;
      }
    </style>
  </head>
  <body>
    <h1>${code}</h1>
    <pre>${escaped}</pre>
  </body>
</html>`;
}
