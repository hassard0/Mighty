// v0.33 T3 — playground bootstrap.
//
// Wires:
//   examples + share decode  -> initial editor content
//   Run button + Ctrl+Enter   -> runner.run()
//   Check button              -> runner.check()
//   Format button             -> placeholder (mty-fmt is the next wasm export)
//   Save & Share              -> share.encode + clipboard
//   Output tabs               -> stdout / diagnostics / trace
//
// All UI state lives on plain DOM elements; no framework. Adding one
// would slow time-to-interactive without buying us anything at this
// surface area.

import { createEditor, MightyEditor } from "./editor.ts";
import {
  Diagnostic,
  renderDiagnostics,
  toMarkers,
} from "./diagnostics.ts";
import {
  DEFAULT_EXAMPLE_ID,
  EXAMPLES,
  Example,
  findExample,
} from "./examples.ts";
import { makeRunner, Runner } from "./runner.ts";
import {
  copyToClipboard,
  decodeShareState,
  encodeShareLink,
} from "./share.ts";

// ---------------------------------------------------------------------------
// DOM lookups
// ---------------------------------------------------------------------------

function $(id: string): HTMLElement {
  const el = document.getElementById(id);
  if (!el) throw new Error(`#${id} missing in index.html`);
  return el;
}

const editorHost = $("editor");
const btnRun = $("btn-run") as HTMLButtonElement;
const btnCheck = $("btn-check") as HTMLButtonElement;
const btnShare = $("btn-share") as HTMLButtonElement;
const btnFormat = $("btn-format") as HTMLButtonElement;
const picker = $("example-picker") as HTMLSelectElement;
const statusPill = $("status-pill");
const backendModePill = $("backend-mode");
const outStdout = $("output-stdout") as HTMLPreElement;
const outDiag = $("output-diagnostics");
const outTrace = $("output-trace") as HTMLPreElement;
const tabs = Array.from(
  document.querySelectorAll<HTMLButtonElement>(".output-tabs__tab"),
);

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

let editor: MightyEditor;
let runner: Runner;

const initialState = decodeShareState(window.location.hash);
const initialExample: Example =
  (initialState.exampleId && findExample(initialState.exampleId)) ||
  findExample(DEFAULT_EXAMPLE_ID)!;
const initialSource = initialState.code ?? initialExample.source;

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

function populatePicker() {
  for (const ex of EXAMPLES) {
    const opt = document.createElement("option");
    opt.value = ex.id;
    opt.textContent = ex.title;
    picker.appendChild(opt);
  }
  picker.value = initialExample.id;
}

function setStatus(state: "idle" | "running" | "ok" | "error", text: string) {
  statusPill.dataset.state = state;
  statusPill.textContent = text;
}

function activateTab(tab: string) {
  for (const t of tabs) {
    const active = t.dataset.tab === tab;
    t.setAttribute("aria-selected", active ? "true" : "false");
  }
  for (const pane of [outStdout, outDiag, outTrace]) {
    pane.hidden = pane.dataset.tab !== tab;
  }
}

function setOutput(stdout: string, diags: Diagnostic[], trace: string) {
  outStdout.textContent = stdout || "// (no stdout)";
  renderDiagnostics(outDiag, diags, editor.getValue());
  outTrace.textContent = trace || "// (no trace — wasm backend not loaded)";
}

function showToast(text: string) {
  const t = document.createElement("div");
  t.className = "toast";
  t.textContent = text;
  document.body.appendChild(t);
  requestAnimationFrame(() => t.classList.add("toast--show"));
  setTimeout(() => {
    t.classList.remove("toast--show");
    setTimeout(() => t.remove(), 250);
  }, 1800);
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

async function doRun() {
  setStatus("running", "running");
  btnRun.disabled = true;
  btnCheck.disabled = true;
  try {
    const result = await runner.run(editor.getValue());
    editor.setMarkers(toMarkers(result.diagnostics));
    setOutput(result.stdout, result.diagnostics, result.trace);
    if (result.ok) {
      setStatus("ok", "ok");
      activateTab(result.diagnostics.length > 0 ? "diagnostics" : "stdout");
    } else {
      setStatus("error", "error");
      activateTab("diagnostics");
    }
  } catch (e) {
    setStatus("error", "runner failure");
    setOutput(
      "",
      [
        {
          code: "PG0001",
          severity: "error",
          message: `runner error: ${
            e instanceof Error ? e.message : String(e)
          }`,
          primary: { span: { start: 0, end: 0, line: 1, col: 1 }, message: "" },
          secondary: [],
          notes: [],
          helps: ["see browser console for the stack"],
          fixes: [],
        },
      ],
      "",
    );
    activateTab("diagnostics");
  } finally {
    btnRun.disabled = false;
    btnCheck.disabled = false;
  }
}

async function doCheck() {
  setStatus("running", "checking");
  btnRun.disabled = true;
  btnCheck.disabled = true;
  try {
    const result = await runner.check(editor.getValue());
    editor.setMarkers(toMarkers(result.diagnostics));
    setOutput("", result.diagnostics, "// run for trace");
    setStatus(result.ok ? "ok" : "error", result.ok ? "clean" : "errors");
    activateTab("diagnostics");
  } finally {
    btnRun.disabled = false;
    btnCheck.disabled = false;
  }
}

async function doShare() {
  const url = encodeShareLink(editor.getValue());
  history.replaceState(null, "", url);
  const copied = await copyToClipboard(url);
  showToast(copied ? "link copied to clipboard" : "permalink in address bar");
}

function doFormat() {
  // mty-fmt is a separate wasm export — v0.34 follow-up. Until then we
  // do a tiny no-op pass: ensure trailing newline + collapse trailing
  // whitespace per line. Better than nothing; honest about its scope.
  const src = editor.getValue();
  const normalised = src
    .split("\n")
    .map((l) => l.replace(/[ \t]+$/, ""))
    .join("\n")
    .replace(/\n*$/, "\n");
  editor.setValue(normalised);
  showToast("light format applied — full mty-fmt lands in v0.34");
}

function loadExample(id: string) {
  const ex = findExample(id);
  if (!ex) return;
  editor.setValue(ex.source);
  editor.clearMarkers();
  setOutput("", [], "");
  setStatus("idle", "ready");
  activateTab("stdout");
  // Update the URL so the example deep-links.
  history.replaceState(null, "", `${window.location.pathname}#example=${id}`);
}

// ---------------------------------------------------------------------------
// Wire-up
// ---------------------------------------------------------------------------

async function bootstrap() {
  editor = createEditor(editorHost, initialSource);
  editor.onCmdEnter(() => {
    void doRun();
  });

  runner = makeRunner();
  backendModePill.textContent = runner.mode;

  await runner.init();

  populatePicker();
  picker.addEventListener("change", () => loadExample(picker.value));
  btnRun.addEventListener("click", () => void doRun());
  btnCheck.addEventListener("click", () => void doCheck());
  btnShare.addEventListener("click", () => void doShare());
  btnFormat.addEventListener("click", doFormat);

  for (const t of tabs) {
    t.addEventListener("click", () => activateTab(t.dataset.tab!));
  }

  setStatus("idle", "ready");
  editor.focus();
}

bootstrap().catch((e) => {
  // Last-ditch surface. If the editor never came up, log to body so the
  // user gets something better than a blank screen.
  console.error(e);
  const pre = document.createElement("pre");
  pre.style.color = "#ff7a82";
  pre.style.padding = "16px";
  pre.style.fontFamily = "ui-monospace, Menlo, Consolas, monospace";
  pre.textContent = `playground bootstrap failed:\n${
    e instanceof Error ? e.stack || e.message : String(e)
  }`;
  document.body.appendChild(pre);
});
