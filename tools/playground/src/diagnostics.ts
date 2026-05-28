// v0.33 T3 — structured fix-envelope renderer.
//
// T4 is shipping the production fix-envelope shape in parallel. This
// module commits to the consumer contract: whatever T4 emits from the
// mty-cli wasm target lands here as `Diagnostic[]` and we render it.
//
// Contract (subject to T4 ratification — keep this in sync):
//
//   type Severity = "error" | "warning" | "note" | "help";
//
//   interface Span {
//     start: number;   // byte offset into the source
//     end: number;
//     line: number;    // 1-based, computed by the playground from `start`
//     col: number;     // 1-based
//   }
//
//   interface Label { span: Span; message: string; }
//
//   interface Fix {
//     title: string;          // "replace `Foo` with `Bar`"
//     /** Source range to replace. Optional — purely-textual help fixes
//      *  may have no range (the title is the actionable instruction). */
//     span?: Span;
//     replacement?: string;
//   }
//
//   interface Diagnostic {
//     code: string;           // e.g. "MT4099"
//     severity: Severity;
//     message: string;        // top-line, no source quote
//     primary: Label;
//     secondary: Label[];
//     notes: string[];        // "= note: ..." lines
//     helps: string[];        // "= help: ..." lines
//     fixes: Fix[];           // structured machine-applicable fix envelopes
//   }
//
// When T4 lands, runner.ts converts the wasm-side payload into this
// shape and we render the same UI.

import { MarkerSeverity } from "./editor.ts";
import type * as monaco from "monaco-editor";

export type Severity = "error" | "warning" | "note" | "help";

export interface Span {
  start: number;
  end: number;
  line: number;
  col: number;
}

export interface Label {
  span: Span;
  message: string;
}

export interface Fix {
  title: string;
  span?: Span;
  replacement?: string;
}

export interface Diagnostic {
  code: string;
  severity: Severity;
  message: string;
  primary: Label;
  secondary: Label[];
  notes: string[];
  helps: string[];
  fixes: Fix[];
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/** Compute line/col from byte offset. Tolerates out-of-range gracefully. */
export function spanFromOffset(
  source: string,
  start: number,
  end: number,
): Span {
  const safeStart = Math.max(0, Math.min(start, source.length));
  const safeEnd = Math.max(safeStart, Math.min(end, source.length));
  let line = 1;
  let col = 1;
  for (let i = 0; i < safeStart; i++) {
    if (source.charCodeAt(i) === 10) {
      line += 1;
      col = 1;
    } else {
      col += 1;
    }
  }
  return { start: safeStart, end: safeEnd, line, col };
}

/** Pull the source line containing `span.start` plus a caret underline.
 *  Mirrors what ariadne emits in the CLI. */
function snippetFor(source: string, span: Span): string {
  const lines = source.split("\n");
  const lineIdx = span.line - 1;
  if (lineIdx < 0 || lineIdx >= lines.length) return "";
  const line = lines[lineIdx];
  const caretCount = Math.max(1, span.end - span.start);
  const pad = " ".repeat(Math.max(0, span.col - 1));
  const caret = "^".repeat(Math.min(caretCount, Math.max(1, line.length - span.col + 1)));
  const lineLabel = String(span.line).padStart(3, " ");
  return `${lineLabel} | ${line}\n    | ${pad}${caret}`;
}

function renderOne(diag: Diagnostic, source: string): HTMLElement {
  const wrap = document.createElement("article");
  wrap.className = "diag";
  wrap.dataset.severity = diag.severity;

  const head = document.createElement("div");
  head.className = "diag__head";
  const code = document.createElement("span");
  code.className = "diag__code";
  code.textContent = `${diag.severity}[${diag.code}]`;
  head.appendChild(code);
  const loc = document.createElement("span");
  loc.className = "diag__loc";
  loc.textContent = `at line ${diag.primary.span.line}:${diag.primary.span.col}`;
  head.appendChild(loc);
  wrap.appendChild(head);

  const msg = document.createElement("p");
  msg.className = "diag__message";
  msg.textContent = diag.message;
  wrap.appendChild(msg);

  const snippet = snippetFor(source, diag.primary.span);
  if (snippet) {
    const pre = document.createElement("pre");
    pre.className = "diag__snippet";
    pre.innerHTML = snippet
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/\^+/g, (m) => `<span class="diag__caret">${m}</span>`);
    wrap.appendChild(pre);
    const primaryMsg = diag.primary.message?.trim();
    if (primaryMsg) {
      const tag = document.createElement("p");
      tag.className = "diag__message";
      tag.textContent = `^ ${primaryMsg}`;
      wrap.appendChild(tag);
    }
  }

  if (diag.fixes.length > 0) {
    const ul = document.createElement("ul");
    ul.className = "diag__fixes";
    for (const fix of diag.fixes) {
      const li = document.createElement("li");
      li.className = "diag__fix";
      li.textContent = fix.title;
      ul.appendChild(li);
    }
    wrap.appendChild(ul);
  }

  if (diag.notes.length > 0 || diag.helps.length > 0) {
    const ul = document.createElement("ul");
    ul.className = "diag__notes";
    for (const n of diag.notes) {
      const li = document.createElement("li");
      li.textContent = `note: ${n}`;
      ul.appendChild(li);
    }
    for (const h of diag.helps) {
      const li = document.createElement("li");
      li.textContent = `help: ${h}`;
      ul.appendChild(li);
    }
    wrap.appendChild(ul);
  }

  return wrap;
}

export function renderDiagnostics(
  host: HTMLElement,
  diags: Diagnostic[],
  source: string,
) {
  host.replaceChildren();
  if (diags.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.textContent = "no diagnostics — clean parse + typecheck.";
    host.appendChild(empty);
    return;
  }
  for (const d of diags) {
    host.appendChild(renderOne(d, source));
  }
}

/** Convert our Diagnostic[] into Monaco markers for the editor gutter. */
export function toMarkers(
  diags: Diagnostic[],
): monaco.editor.IMarkerData[] {
  return diags.map((d) => ({
    severity: severityToMarker(d.severity),
    message: `[${d.code}] ${d.message}`,
    startLineNumber: d.primary.span.line,
    startColumn: d.primary.span.col,
    endLineNumber: d.primary.span.line,
    endColumn: d.primary.span.col + Math.max(1, d.primary.span.end - d.primary.span.start),
  }));
}

function severityToMarker(s: Severity): monaco.MarkerSeverity {
  switch (s) {
    case "error":
      return MarkerSeverity.Error;
    case "warning":
      return MarkerSeverity.Warning;
    case "note":
      return MarkerSeverity.Info;
    case "help":
      return MarkerSeverity.Hint;
  }
}
