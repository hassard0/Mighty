// Cost side-panel webview.
//
// Replaces the terminal-based `Mighty: Inspect cost` command (now
// renamed to "Mighty: Inspect cost (terminal)") with a richer panel.
//
// The panel is HTML+CSS only — no JS frameworks, no Chart.js, no
// dynamic message passing back to the extension host. We re-render
// the full body every 30s by shelling out to:
//
//   mty inspect --cost --json --since 30d --by all
//
// and translating the response into one of three sections:
//
//   1. Top: four summary cards (today, 7d, 30d, all-time).
//   2. Middle: a per-provider/model breakdown rendered as plain HTML
//      <div> bars sized via inline `width: X%` — VS Code theme variables
//      do the colouring.
//   3. Bottom: a top-10 most-expensive calls table.
//
// Theme-awareness: every colour we emit comes from `var(--vscode-…)`
// CSS variables. There's no @vscode/webview-ui-toolkit script include
// because we don't need its widgets; using its variable names is
// enough for a faithful Light/Dark/HC re-skin.

import * as cp from "child_process";
import * as vscode from "vscode";

const REFRESH_INTERVAL_MS = 30_000;

interface CostBucket {
  label: string;
  call_count: number;
  total_cost_cents: number;
}

interface TopCall {
  ts: string;
  provider: string;
  model: string;
  agent?: string;
  cost_cents: number;
  latency_ms?: number;
}

interface CostJson {
  today: CostBucket;
  last_7d: CostBucket;
  last_30d: CostBucket;
  all_time: CostBucket;
  by_provider: CostBucket[];
  by_model: CostBucket[];
  top_calls: TopCall[];
}

/**
 * Manages the lifecycle of the cost panel — registers the command,
 * creates/reuses the webview, drives the 30s refresh loop, and tears
 * down the timer on dispose.
 */
export class CostPanelController implements vscode.Disposable {
  private panel: vscode.WebviewPanel | undefined;
  private timer: NodeJS.Timeout | undefined;
  private disposed = false;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly mtyPath: string,
  ) {}

  /** Open (or reveal) the panel. Called from the registered command. */
  show(): void {
    if (this.panel) {
      this.panel.reveal(vscode.ViewColumn.Beside);
      void this.refresh();
      return;
    }

    this.panel = vscode.window.createWebviewPanel(
      "mighty.costPanel",
      "Mighty: Cost",
      vscode.ViewColumn.Beside,
      {
        enableScripts: false,
        retainContextWhenHidden: true,
      },
    );

    this.panel.onDidDispose(() => {
      this.panel = undefined;
      if (this.timer) {
        clearInterval(this.timer);
        this.timer = undefined;
      }
    });

    this.panel.webview.html = renderLoadingHtml();

    void this.refresh();
    this.timer = setInterval(() => {
      void this.refresh();
    }, REFRESH_INTERVAL_MS);

    this.context.subscriptions.push(this.panel);
  }

  private async refresh(): Promise<void> {
    if (this.disposed || !this.panel) return;
    try {
      const data = await this.fetch();
      this.panel.webview.html = renderHtml(data);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      this.panel.webview.html = renderErrorHtml(msg);
    }
  }

  private fetch(): Promise<CostJson> {
    return new Promise((resolve, reject) => {
      const child = cp.spawn(
        this.mtyPath,
        [
          "inspect",
          "--cost",
          "--json",
          "--since",
          "30d",
          "--by",
          "all",
        ],
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
          reject(new Error(`mty inspect exit ${code}: ${stderr}`));
          return;
        }
        try {
          resolve(coerceCostJson(JSON.parse(stdout)));
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
    this.panel?.dispose();
    this.panel = undefined;
  }
}

// ----------------------------------------------------------------------------
// Rendering
// ----------------------------------------------------------------------------

function renderLoadingHtml(): string {
  return shell(
    `<section class="empty">
       <h2>Loading cost data…</h2>
       <p>Running <code>mty inspect --cost --json</code>.</p>
     </section>`,
  );
}

function renderErrorHtml(msg: string): string {
  const safe = escapeHtml(msg);
  return shell(
    `<section class="empty">
       <h2>Cost data unavailable</h2>
       <p>The <code>mty inspect</code> command failed:</p>
       <pre>${safe}</pre>
       <p>
         Set <code>mighty.server.path</code> to your <code>mty</code>
         binary, or run <code>mty test --eval</code> at least once so
         observations are recorded.
       </p>
     </section>`,
  );
}

function renderHtml(data: CostJson): string {
  return shell(`
    ${renderSummaryCards(data)}
    ${renderBreakdown("Spend by provider", data.by_provider)}
    ${renderBreakdown("Spend by model", data.by_model)}
    ${renderTopCalls(data.top_calls)}
    <footer>
      <span>Auto-refreshes every ${REFRESH_INTERVAL_MS / 1000}s · source: <code>~/.mty/observations.sqlite</code></span>
    </footer>
  `);
}

function renderSummaryCards(data: CostJson): string {
  const cards = [
    ["Today", data.today],
    ["Last 7 days", data.last_7d],
    ["Last 30 days", data.last_30d],
    ["All time", data.all_time],
  ] as const;

  const items = cards
    .map(([label, bucket]) => {
      const dollars = (bucket.total_cost_cents / 100).toFixed(2);
      const calls =
        bucket.call_count === 1 ? "1 call" : `${bucket.call_count} calls`;
      return `
        <div class="card">
          <div class="card-label">${escapeHtml(label)}</div>
          <div class="card-value">$${dollars}</div>
          <div class="card-sub">${escapeHtml(calls)}</div>
        </div>
      `;
    })
    .join("");

  return `<section class="cards">${items}</section>`;
}

function renderBreakdown(title: string, buckets: CostBucket[]): string {
  if (!buckets.length) {
    return `
      <section class="breakdown">
        <h2>${escapeHtml(title)}</h2>
        <p class="muted">No data recorded.</p>
      </section>
    `;
  }
  const max = Math.max(...buckets.map((b) => b.total_cost_cents), 1);
  const rows = buckets
    .slice(0, 8)
    .map((b) => {
      const dollars = (b.total_cost_cents / 100).toFixed(2);
      const pct = Math.min(100, (b.total_cost_cents / max) * 100);
      return `
        <div class="bar-row">
          <div class="bar-label">${escapeHtml(b.label)}</div>
          <div class="bar-track">
            <div class="bar-fill" style="width: ${pct.toFixed(1)}%"></div>
          </div>
          <div class="bar-value">$${dollars}</div>
        </div>
      `;
    })
    .join("");

  return `
    <section class="breakdown">
      <h2>${escapeHtml(title)}</h2>
      <div class="bars">${rows}</div>
    </section>
  `;
}

function renderTopCalls(calls: TopCall[]): string {
  if (!calls.length) {
    return `
      <section class="top-calls">
        <h2>Top 10 most expensive calls</h2>
        <p class="muted">No calls recorded.</p>
      </section>
    `;
  }
  const rows = calls
    .slice(0, 10)
    .map((c) => {
      const dollars = (c.cost_cents / 100).toFixed(4);
      const latency =
        typeof c.latency_ms === "number" ? `${c.latency_ms} ms` : "—";
      return `
        <tr>
          <td>${escapeHtml(c.ts)}</td>
          <td>${escapeHtml(c.provider)}</td>
          <td>${escapeHtml(c.model)}</td>
          <td>${escapeHtml(c.agent ?? "—")}</td>
          <td class="num">$${dollars}</td>
          <td class="num">${escapeHtml(latency)}</td>
        </tr>
      `;
    })
    .join("");
  return `
    <section class="top-calls">
      <h2>Top 10 most expensive calls</h2>
      <table>
        <thead>
          <tr>
            <th>When</th>
            <th>Provider</th>
            <th>Model</th>
            <th>Agent</th>
            <th class="num">Cost</th>
            <th class="num">Latency</th>
          </tr>
        </thead>
        <tbody>${rows}</tbody>
      </table>
    </section>
  `;
}

function shell(body: string): string {
  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta
      http-equiv="Content-Security-Policy"
      content="default-src 'none'; style-src 'unsafe-inline';"
    />
    <title>Mighty: Cost</title>
    <style>${PANEL_CSS}</style>
  </head>
  <body>
    <header>
      <h1>Mighty · LLM cost</h1>
    </header>
    <main>${body}</main>
  </body>
</html>`;
}

const PANEL_CSS = `
:root {
  color-scheme: light dark;
}

body {
  margin: 0;
  padding: 16px 20px 28px;
  background: var(--vscode-editor-background);
  color: var(--vscode-editor-foreground);
  font-family: var(--vscode-font-family, "Segoe UI", sans-serif);
  font-size: var(--vscode-font-size, 13px);
}

header h1 {
  margin: 0 0 16px;
  font-size: 1.4em;
  font-weight: 600;
  color: var(--vscode-textLink-foreground);
}

h2 {
  margin: 18px 0 10px;
  font-size: 1.05em;
  font-weight: 600;
}

code {
  font-family: var(--vscode-editor-font-family, monospace);
  background: var(--vscode-textBlockQuote-background, transparent);
  padding: 0 4px;
  border-radius: 3px;
}

.empty {
  padding: 32px 0;
  text-align: center;
  color: var(--vscode-descriptionForeground);
}

.muted {
  color: var(--vscode-descriptionForeground);
  font-style: italic;
}

/* ---- Summary cards ---- */

.cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
  gap: 12px;
  margin-bottom: 12px;
}

.card {
  background: var(--vscode-editorWidget-background, rgba(127, 127, 127, 0.08));
  border: 1px solid var(--vscode-editorWidget-border, rgba(127, 127, 127, 0.2));
  border-radius: 6px;
  padding: 12px 14px;
}

.card-label {
  font-size: 0.85em;
  color: var(--vscode-descriptionForeground);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.card-value {
  font-size: 1.7em;
  font-weight: 600;
  color: var(--vscode-textLink-foreground);
  margin: 4px 0 2px;
}

.card-sub {
  font-size: 0.85em;
  color: var(--vscode-descriptionForeground);
}

/* ---- Breakdown bars ---- */

.breakdown {
  margin-top: 18px;
}

.bars {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.bar-row {
  display: grid;
  grid-template-columns: minmax(140px, 0.4fr) 1fr 90px;
  align-items: center;
  gap: 12px;
}

.bar-label {
  font-family: var(--vscode-editor-font-family, monospace);
  font-size: 0.9em;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.bar-track {
  background: var(--vscode-input-background, rgba(127, 127, 127, 0.15));
  border: 1px solid var(--vscode-input-border, transparent);
  border-radius: 4px;
  height: 14px;
  position: relative;
  overflow: hidden;
}

.bar-fill {
  background: var(--vscode-progressBar-background, var(--vscode-textLink-foreground));
  height: 100%;
  border-radius: 3px;
  transition: width 0.2s ease;
}

.bar-value {
  text-align: right;
  font-variant-numeric: tabular-nums;
  font-size: 0.9em;
}

/* ---- Top-calls table ---- */

.top-calls {
  margin-top: 22px;
}

table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.9em;
}

th, td {
  text-align: left;
  padding: 6px 8px;
  border-bottom: 1px solid var(--vscode-editorWidget-border, rgba(127, 127, 127, 0.2));
}

th {
  font-weight: 600;
  color: var(--vscode-descriptionForeground);
  text-transform: uppercase;
  letter-spacing: 0.03em;
  font-size: 0.8em;
}

td.num, th.num {
  text-align: right;
  font-variant-numeric: tabular-nums;
}

tr:hover td {
  background: var(--vscode-list-hoverBackground, rgba(127, 127, 127, 0.08));
}

footer {
  margin-top: 28px;
  font-size: 0.8em;
  color: var(--vscode-descriptionForeground);
}

pre {
  white-space: pre-wrap;
  word-wrap: break-word;
  background: var(--vscode-textBlockQuote-background, transparent);
  border-left: 3px solid var(--vscode-textBlockQuote-border, #888);
  padding: 8px 12px;
  text-align: left;
}
`;

// ----------------------------------------------------------------------------
// Defensive coercion — `mty inspect --cost --json` is best-effort about
// shape; we fill in zero buckets for anything missing.
// ----------------------------------------------------------------------------

function coerceCostJson(raw: unknown): CostJson {
  const obj = (raw && typeof raw === "object" ? raw : {}) as Record<
    string,
    unknown
  >;
  return {
    today: coerceBucket(obj.today),
    last_7d: coerceBucket(obj.last_7d),
    last_30d: coerceBucket(obj.last_30d),
    all_time: coerceBucket(obj.all_time),
    by_provider: coerceBucketArray(obj.by_provider),
    by_model: coerceBucketArray(obj.by_model),
    top_calls: coerceTopCalls(obj.top_calls),
  };
}

function coerceBucket(raw: unknown): CostBucket {
  const obj = (raw && typeof raw === "object" ? raw : {}) as Record<
    string,
    unknown
  >;
  return {
    label: typeof obj.label === "string" ? obj.label : "",
    call_count: typeof obj.call_count === "number" ? obj.call_count : 0,
    total_cost_cents:
      typeof obj.total_cost_cents === "number" ? obj.total_cost_cents : 0,
  };
}

function coerceBucketArray(raw: unknown): CostBucket[] {
  if (!Array.isArray(raw)) return [];
  return raw.map((entry) => coerceBucket(entry));
}

function coerceTopCalls(raw: unknown): TopCall[] {
  if (!Array.isArray(raw)) return [];
  return raw.map((entry) => {
    const obj = (entry && typeof entry === "object" ? entry : {}) as Record<
      string,
      unknown
    >;
    return {
      ts: typeof obj.ts === "string" ? obj.ts : "",
      provider: typeof obj.provider === "string" ? obj.provider : "",
      model: typeof obj.model === "string" ? obj.model : "",
      agent: typeof obj.agent === "string" ? obj.agent : undefined,
      cost_cents: typeof obj.cost_cents === "number" ? obj.cost_cents : 0,
      latency_ms:
        typeof obj.latency_ms === "number" ? obj.latency_ms : undefined,
    };
  });
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}
