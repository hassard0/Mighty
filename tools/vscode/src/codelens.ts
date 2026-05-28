// CodeLens provider — cost annotations above call sites.
//
// For every line in a Mighty file that contains a Mighty "call site" we
// recognise (an `@tool(` decorator, a `swarm(` block, a `Member.<vendor>(`
// constructor, or a `.ask(` invocation), we emit a CodeLens of the form:
//
//     $0.04 total · 12 calls · last 24h
//
// or, if no observation rows match that file, the empty-state lens:
//
//     $0.00 · no calls recorded
//
// Clicking the lens runs `mty inspect --cost --top 10 --by agent <path>`
// in a terminal — i.e. the same data, scoped to the source file.
//
// Implementation notes
// --------------------
//
// 1. The lens "command" is `mighty.inspectCostForFile` — it shells out
//    to the `mty` CLI with the active file path. We register it in
//    extension.ts to keep the activation surface in one place.
//
// 2. CodeLens runs on the document's text + a cached cost-by-file
//    snapshot. The snapshot is refreshed every 60s by polling
//    `mty inspect --cost --since 24h --by file --json`. We invalidate
//    eagerly on document save (so editing + saving feels responsive).
//
// 3. If the `mty` CLI is missing or the SQLite DB is empty, the
//    snapshot map is empty and every site renders the empty-state
//    lens — no errors are surfaced, because absence-of-data is the
//    normal first-run state for users who haven't run anything yet.
//
// 4. The regex set is intentionally permissive — a comment line
//    containing `swarm(` will get a lens. That's an acceptable
//    false-positive rate; the alternative (full lex) would require
//    pulling the tree-sitter binding in, which is out of scope for
//    this provider.

import * as cp from "child_process";
import * as vscode from "vscode";

/** Command ID for the lens click target. */
export const COST_LENS_COMMAND = "mighty.inspectCostForFile";

/**
 * Patterns that mark a Mighty cost-incurring call site. Order does not
 * matter; the provider walks each line once and emits a lens at the
 * first match.
 */
const CALL_SITE_PATTERNS: RegExp[] = [
  /@tool\s*\(/,
  /\bswarm\s*\(/,
  /\bMember\.anthropic\s*\(/,
  /\bMember\.openai\s*\(/,
  /\bMember\.gemini\s*\(/,
  /\bMember\.bedrock\s*\(/,
  /\.ask\s*\(/,
];

/** A single bucket of cost data, scoped by file path. */
interface FileCostRow {
  file: string;
  call_count: number;
  total_cost_cents: number;
}

/** Top-level shape returned by `mty inspect --cost --by file --json`. */
interface FileCostSnapshot {
  rows: FileCostRow[];
}

/**
 * Polls the cost snapshot on a timer, exposes a current() accessor,
 * and lets callers force a refresh. Held as a singleton on the
 * extension activation context.
 */
export class CostSnapshotCache implements vscode.Disposable {
  private snapshot: Map<string, FileCostRow> = new Map();
  private timer: NodeJS.Timeout | undefined;
  private disposed = false;
  private readonly emitter = new vscode.EventEmitter<void>();

  /** Fires when the snapshot has been refreshed. */
  readonly onDidChange: vscode.Event<void> = this.emitter.event;

  constructor(private readonly mtyPath: string) {}

  start(intervalMs = 60_000): void {
    void this.refresh();
    this.timer = setInterval(() => {
      void this.refresh();
    }, intervalMs);
  }

  /**
   * Look up cost data for an absolute file path. Comparison is
   * case-insensitive on Windows and exact on every other OS; we also
   * fall back to a basename match because `mty inspect` may report
   * paths relative to the workspace root.
   */
  forFile(absPath: string): FileCostRow | undefined {
    if (this.snapshot.size === 0) return undefined;
    const norm = normalisePath(absPath);
    const hit = this.snapshot.get(norm);
    if (hit) return hit;

    const base = basename(norm);
    for (const [key, row] of this.snapshot) {
      if (basename(key) === base) return row;
    }
    return undefined;
  }

  /** Force a fresh poll — used on document save. */
  async refresh(): Promise<void> {
    if (this.disposed) return;
    try {
      const next = await this.fetchSnapshot();
      this.snapshot = new Map(
        next.rows.map((row) => [normalisePath(row.file), row] as const),
      );
      this.emitter.fire();
    } catch {
      // Best-effort. Leave the previous snapshot in place rather than
      // wiping it on a transient CLI failure.
    }
  }

  private fetchSnapshot(): Promise<FileCostSnapshot> {
    return new Promise((resolve, reject) => {
      const child = cp.spawn(
        this.mtyPath,
        ["inspect", "--cost", "--since", "24h", "--by", "file", "--json"],
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
          reject(new Error(`mty inspect --by file exit ${code}: ${stderr}`));
          return;
        }
        try {
          const parsed = JSON.parse(stdout) as Partial<FileCostSnapshot>;
          resolve({ rows: Array.isArray(parsed.rows) ? parsed.rows : [] });
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
    this.emitter.dispose();
  }
}

/**
 * The CodeLens provider itself. Pure-text scan over document lines —
 * we never block on the CLI here because VS Code calls
 * provideCodeLenses on every visible-range change.
 */
export class MightyCostCodeLensProvider implements vscode.CodeLensProvider {
  private readonly emitter = new vscode.EventEmitter<void>();
  readonly onDidChangeCodeLenses: vscode.Event<void> = this.emitter.event;

  constructor(private readonly cache: CostSnapshotCache) {
    // Refresh lenses whenever the snapshot updates.
    this.cache.onDidChange(() => this.emitter.fire());
  }

  provideCodeLenses(
    document: vscode.TextDocument,
    token: vscode.CancellationToken,
  ): vscode.CodeLens[] {
    if (document.languageId !== "mighty") return [];

    const row = this.cache.forFile(document.uri.fsPath);
    const lenses: vscode.CodeLens[] = [];

    const text = document.getText();
    const lines = text.split(/\r?\n/);

    // Per-line cost is a fair approximation when we have no per-call
    // site granularity yet — every recognised lens on the file gets
    // the same "file total" until the v0.33 per-span observation
    // table lands. We still surface it to make the empty/non-empty
    // distinction visible.
    const lensTitle = formatLensTitle(row);

    for (let lineNo = 0; lineNo < lines.length; lineNo++) {
      if (token.isCancellationRequested) break;
      const line = lines[lineNo];
      if (!hasCallSite(line)) continue;

      const range = new vscode.Range(
        new vscode.Position(lineNo, 0),
        new vscode.Position(lineNo, 0),
      );
      lenses.push(
        new vscode.CodeLens(range, {
          title: lensTitle,
          command: COST_LENS_COMMAND,
          arguments: [document.uri.fsPath],
          tooltip:
            "Open `mty inspect --cost --top 10 --by agent <file>` for this file.",
        }),
      );
    }

    return lenses;
  }

  /** Listener hook for document save / external refresh. */
  fireChanged(): void {
    this.emitter.fire();
  }

  dispose(): void {
    this.emitter.dispose();
  }
}

function hasCallSite(line: string): boolean {
  for (const re of CALL_SITE_PATTERNS) {
    if (re.test(line)) return true;
  }
  return false;
}

function formatLensTitle(row: FileCostRow | undefined): string {
  if (!row || row.call_count === 0) {
    return "$(graph-line) $0.00 · no calls recorded";
  }
  const dollars = (row.total_cost_cents / 100).toFixed(2);
  const calls = row.call_count === 1 ? "1 call" : `${row.call_count} calls`;
  return `$(graph-line) $${dollars} total · ${calls} · last 24h`;
}

function normalisePath(p: string): string {
  // VS Code returns paths with the OS-native separator. Normalise to
  // forward slashes + lowercase the drive on Windows so the cache key
  // is comparable to whatever shape `mty inspect` reports.
  let out = p.replace(/\\/g, "/");
  if (/^[a-z]:\//i.test(out)) {
    out = out[0].toLowerCase() + out.slice(1);
  }
  return out;
}

function basename(p: string): string {
  const slash = p.lastIndexOf("/");
  return slash < 0 ? p : p.slice(slash + 1);
}
