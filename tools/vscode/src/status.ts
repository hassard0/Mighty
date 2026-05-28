// Cost status-bar item.
//
// Shows today's LLM spend in the bottom-right of the VS Code status
// bar. Refreshes every `mighty.costStatusBar.refreshSeconds` seconds
// (default 30) by shelling out to:
//
//   mty inspect --cost --since 24h --json
//
// We deliberately do NOT open the SQLite directly — the CLI is the
// canonical reader, ships with the toolchain, and already handles
// missing-DB and schema-migration edge cases. If the CLI is absent
// (or returns an error), the bar gracefully degrades to `$0.00`
// rather than going red.
//
// Clicking the item invokes `mighty.inspectCost`, which opens the
// full `mty inspect --cost` table in a terminal.

import * as cp from "child_process";
import * as vscode from "vscode";

const COMMAND_ID = "mighty.inspectCost";

interface CostSnapshot {
  call_count: number;
  total_cost_cents: number;
}

export class CostStatusBar implements vscode.Disposable {
  private item: vscode.StatusBarItem;
  private timer: NodeJS.Timeout | undefined;
  private disposed = false;

  constructor(
    context: vscode.ExtensionContext,
    private readonly mtyPath: string,
  ) {
    this.item = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Right,
      100,
    );
    this.item.command = COMMAND_ID;
    this.item.text = "$(graph-line) Mighty: $0.00";
    this.item.tooltip = "Click for the full cost breakdown.";
    this.item.show();
    context.subscriptions.push(this.item);
  }

  /** Start the refresh loop. Runs an immediate refresh, then on a timer. */
  start(): void {
    void this.refresh();
    const seconds = vscode.workspace
      .getConfiguration("mighty")
      .get<number>("costStatusBar.refreshSeconds", 30);
    const intervalMs = Math.max(5, seconds) * 1000;
    this.timer = setInterval(() => {
      void this.refresh();
    }, intervalMs);
  }

  /** One-shot refresh — public so users could bind it to a command later. */
  async refresh(): Promise<void> {
    if (this.disposed) return;
    try {
      const snapshot = await this.fetchSnapshot();
      const dollars = (snapshot.total_cost_cents / 100).toFixed(2);
      this.item.text = `$(graph-line) Mighty: $${dollars} (today)`;
      this.item.tooltip =
        `${snapshot.call_count} LLM call(s) in the last 24h\n` +
        `Click for the full cost breakdown.`;
    } catch {
      // Best-effort. Don't surface errors — the empty state IS the
      // user's signal that no observations have been recorded yet.
      this.item.text = "$(graph-line) Mighty: $0.00";
      this.item.tooltip =
        "No observations recorded yet (or mty inspect unavailable).\n" +
        "Click to open the full cost view.";
    }
  }

  private fetchSnapshot(): Promise<CostSnapshot> {
    return new Promise((resolve, reject) => {
      const child = cp.spawn(
        this.mtyPath,
        ["inspect", "--cost", "--since", "24h", "--json"],
        { shell: false },
      );
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
        if (code !== 0) {
          reject(new Error(`mty inspect --cost exit ${code}: ${stderr}`));
          return;
        }
        try {
          const parsed = JSON.parse(stdout) as Partial<CostSnapshot>;
          resolve({
            call_count:
              typeof parsed.call_count === "number" ? parsed.call_count : 0,
            total_cost_cents:
              typeof parsed.total_cost_cents === "number"
                ? parsed.total_cost_cents
                : 0,
          });
        } catch (e) {
          reject(e);
        }
      });
    });
  }

  dispose(): void {
    this.disposed = true;
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = undefined;
    }
    this.item.dispose();
  }
}
