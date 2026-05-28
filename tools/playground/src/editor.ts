// v0.33 T3 — Monaco editor wrapper for the Mighty playground.
//
// Monaco is a heavy dep but it's the one thing every web IDE user
// already has muscle memory for, and lazy-loading it pays back the
// time-to-interactive cost. We register a minimal Mighty language
// definition here — the full tree-sitter grammar ships separately
// (tools/tree-sitter/) and we'll wire it in v0.34 via a wasm worker.
//
// For now the tokenizer is a Monarch grammar that covers the surface
// the bundled examples need: keywords, types, comments, strings,
// numbers, and the @tool / @effect decorator prefix. Good enough to
// look polished; not aspirationally complete.

import * as monaco from "monaco-editor";

// Workers — Vite ships these as separate ES modules. We only need the
// editor worker because we don't enable TS/CSS/HTML language services.
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";

self.MonacoEnvironment = {
  getWorker: () => new EditorWorker(),
};

const MIGHTY_KEYWORDS = [
  "agent",
  "and",
  "as",
  "async",
  "await",
  "break",
  "case",
  "const",
  "continue",
  "do",
  "effect",
  "else",
  "enum",
  "extern",
  "false",
  "fn",
  "for",
  "if",
  "impl",
  "in",
  "let",
  "match",
  "mod",
  "mut",
  "not",
  "on",
  "or",
  "package",
  "protocol",
  "pub",
  "return",
  "self",
  "Self",
  "struct",
  "trait",
  "true",
  "type",
  "unsafe",
  "use",
  "where",
  "while",
  "yield",
];

const MIGHTY_TYPES = [
  "Bool",
  "Bytes",
  "F32",
  "F64",
  "I32",
  "I64",
  "Result",
  "Str",
  "String",
  "Tainted",
  "U32",
  "U64",
  "Unit",
];

let registered = false;

/** Register the Mighty language once per page. */
function registerMightyLanguage() {
  if (registered) return;
  registered = true;

  monaco.languages.register({ id: "mighty", extensions: [".mty"] });

  monaco.languages.setLanguageConfiguration("mighty", {
    comments: { lineComment: "//", blockComment: ["/*", "*/"] },
    brackets: [
      ["{", "}"],
      ["[", "]"],
      ["(", ")"],
    ],
    autoClosingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
    ],
    surroundingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"' },
    ],
  });

  monaco.languages.setMonarchTokensProvider("mighty", {
    defaultToken: "",
    tokenPostfix: ".mty",
    keywords: MIGHTY_KEYWORDS,
    typeKeywords: MIGHTY_TYPES,
    operators: [
      "=",
      "==",
      "!=",
      "<",
      ">",
      "<=",
      ">=",
      "+",
      "-",
      "*",
      "/",
      "%",
      "+=",
      "-=",
      "*=",
      "/=",
      "&&",
      "||",
      "!",
      "->",
      "=>",
      "::",
      ":",
    ],
    symbols: /[=><!~?:&|+\-*/^%@.]+/,
    tokenizer: {
      root: [
        // Decorators: @tool, @effect, @observe
        [/@[A-Za-z_][\w]*/, "annotation"],
        // Identifiers + keywords
        [
          /[A-Za-z_]\w*/,
          {
            cases: {
              "@keywords": "keyword",
              "@typeKeywords": "type",
              "@default": "identifier",
            },
          },
        ],
        // Whitespace + comments
        { include: "@whitespace" },
        // Numbers
        [/\d+\.\d+([eE][-+]?\d+)?/, "number.float"],
        [/0x[0-9a-fA-F]+/, "number.hex"],
        [/\d+/, "number"],
        // Strings
        [/"([^"\\]|\\.)*$/, "string.invalid"],
        [/"/, "string", "@string"],
        // Brackets + operators
        [/[{}()\[\]]/, "@brackets"],
        [
          /@symbols/,
          { cases: { "@operators": "operator", "@default": "" } },
        ],
      ],
      whitespace: [
        [/[ \t\r\n]+/, ""],
        [/\/\/.*$/, "comment"],
        [/\/\*/, "comment", "@comment"],
      ],
      comment: [
        [/[^/*]+/, "comment"],
        [/\*\//, "comment", "@pop"],
        [/[/*]/, "comment"],
      ],
      string: [
        [/[^\\"]+/, "string"],
        [/\\./, "string.escape"],
        [/"/, "string", "@pop"],
      ],
    },
  });

  // Mighty theme — tuned to match playground.css palette.
  monaco.editor.defineTheme("mighty-dark", {
    base: "vs-dark",
    inherit: true,
    rules: [
      { token: "keyword", foreground: "9b6cff", fontStyle: "bold" },
      { token: "type", foreground: "7aa8ff" },
      { token: "annotation", foreground: "5dd6b8" },
      { token: "string", foreground: "f5c067" },
      { token: "string.escape", foreground: "ff7a82" },
      { token: "number", foreground: "f5c067" },
      { token: "comment", foreground: "5e6371", fontStyle: "italic" },
      { token: "operator", foreground: "9aa0ad" },
    ],
    colors: {
      "editor.background": "#16181f",
      "editor.foreground": "#e6e8ee",
      "editorLineNumber.foreground": "#3a3f4d",
      "editorLineNumber.activeForeground": "#9aa0ad",
      "editor.selectionBackground": "#9b6cff33",
      "editor.lineHighlightBackground": "#1d2029",
      "editorCursor.foreground": "#9b6cff",
      "editorIndentGuide.background": "#20232c",
      "editorIndentGuide.activeBackground": "#2a2e3a",
    },
  });
}

export type MightyEditor = {
  getValue: () => string;
  setValue: (v: string) => void;
  setMarkers: (markers: monaco.editor.IMarkerData[]) => void;
  clearMarkers: () => void;
  focus: () => void;
  onCmdEnter: (cb: () => void) => void;
};

export function createEditor(host: HTMLElement, initial: string): MightyEditor {
  registerMightyLanguage();

  const model = monaco.editor.createModel(initial, "mighty");

  const editor = monaco.editor.create(host, {
    model,
    theme: "mighty-dark",
    automaticLayout: true,
    fontFamily:
      'ui-monospace, "SF Mono", Menlo, Consolas, monospace',
    fontSize: 13.5,
    lineHeight: 20,
    minimap: { enabled: false },
    renderLineHighlight: "line",
    scrollBeyondLastLine: false,
    smoothScrolling: true,
    cursorBlinking: "smooth",
    tabSize: 2,
    insertSpaces: true,
    fixedOverflowWidgets: true,
  });

  return {
    getValue: () => editor.getValue(),
    setValue: (v) => editor.setValue(v),
    setMarkers: (markers) =>
      monaco.editor.setModelMarkers(model, "mighty-diagnostics", markers),
    clearMarkers: () =>
      monaco.editor.setModelMarkers(model, "mighty-diagnostics", []),
    focus: () => editor.focus(),
    onCmdEnter: (cb) => {
      editor.addCommand(
        monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter,
        cb,
      );
    },
  };
}

/** Re-export MonacoMarkerSeverity so diagnostics.ts can map without
 * importing monaco directly. */
export const MarkerSeverity = monaco.MarkerSeverity;
