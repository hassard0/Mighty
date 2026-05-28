// v0.33 T3 — runner.
//
// Two backends behind one interface:
//
//   - MockRunner: pattern-matches the source to produce plausible
//     diagnostics + stdout. Useful right now (no wasm target shipping
//     yet) and as the offline fallback once the real backend lands.
//
//   - WasmRunner: dynamically imports the wasm-bindgen JS shim emitted
//     by `wasm-pack build --target web crates/mty-cli` and calls into
//     the playground binary defined in
//     `crates/mty-cli/src/playground_main.rs`.
//
// The switch is `USE_WASM_BACKEND`, a Vite-time `define` (see
// vite.config.ts). At build time we know whether the wasm artifact
// shipped; the runtime contract is identical either way.

import {
  Diagnostic,
  spanFromOffset,
  Severity,
} from "./diagnostics.ts";

export interface RunResult {
  ok: boolean;
  stdout: string;
  diagnostics: Diagnostic[];
  /** Optional pseudo-trace from the wasm interpreter; empty for the
   *  mock backend. */
  trace: string;
}

export interface CheckResult {
  ok: boolean;
  diagnostics: Diagnostic[];
}

export interface Runner {
  readonly mode: "mock" | "wasm";
  init: () => Promise<void>;
  check: (source: string) => Promise<CheckResult>;
  run: (source: string) => Promise<RunResult>;
}

// ---------------------------------------------------------------------------
// Mock runner
// ---------------------------------------------------------------------------

/** Pattern-driven diagnostics. The list is curated to match the bundled
 *  examples — we want a believable Check experience without compiling
 *  a real Mighty toolchain to wasm. T4 + the real wasm backend replace
 *  this with the live diagnostic stream. */
function mockDiagnostics(source: string): Diagnostic[] {
  const diags: Diagnostic[] = [];

  // ---- Taint flow: Member.ask -> std.fs.write -----------------------------
  //
  // Matches the canonical 33_taint_basics shape. If both a tainted source
  // and a sink appear in the program, the compiler emits MT4099 at the
  // sink. The mock approximates this with an offset scan.
  const askIdx = source.indexOf("m.ask(");
  const writeIdx = source.indexOf("std.fs.write(");
  if (askIdx >= 0 && writeIdx >= 0 && askIdx < writeIdx) {
    const argStart = writeIdx + "std.fs.write(".length;
    // Walk to the second argument (after the first comma) — that's the
    // tainted contents slot.
    let depth = 0;
    let arg2Start = -1;
    for (let i = argStart; i < source.length; i++) {
      const c = source[i];
      if (c === "(" || c === "[") depth++;
      else if (c === ")" || c === "]") depth--;
      else if (c === "," && depth === 0) {
        arg2Start = i + 1;
        while (arg2Start < source.length && source[arg2Start] === " ")
          arg2Start++;
        break;
      }
    }
    if (arg2Start > 0) {
      let arg2End = arg2Start;
      while (
        arg2End < source.length &&
        /[A-Za-z0-9_]/.test(source[arg2End])
      )
        arg2End++;
      const primarySpan = spanFromOffset(source, arg2Start, arg2End);
      diags.push({
        code: "MT4099",
        severity: "error",
        message:
          "tainted value flows to `std.fs.write` (arg #2): the contents argument must be untainted",
        primary: { span: primarySpan, message: "tainted value used here" },
        secondary: [
          {
            span: spanFromOffset(
              source,
              askIdx,
              askIdx + "m.ask(".length,
            ),
            message: "tainted source: `Member.ask` returns `Tainted[Str]`",
          },
        ],
        notes: [
          "tainted values may not reach a documented sink (fs.write, fs.exec, sql.execute, http body)",
        ],
        helps: [
          "untaint via .matches_regex(...), .in_allowlist[Enum](), or .sanitize_with(HtmlEscape | ShellEscape | SqlEscape | PathBoundary(...))",
        ],
        fixes: [
          {
            title:
              "wrap `user_input` with `.sanitize_with(PathBoundary(\"./safe/\"))`",
            span: primarySpan,
            replacement:
              'user_input.sanitize_with(PathBoundary("./safe/"))',
          },
          {
            title:
              "wrap `user_input` with `.matches_regex(\"^[a-z ]{1,80}$\")`",
            span: primarySpan,
            replacement: 'user_input.matches_regex("^[a-z ]{1,80}$")',
          },
        ],
      });
    }
  }

  // ---- Unknown identifier (very common starter mistake) -------------------
  //
  // We deliberately do NOT fire this for `log`, `print`, `Member`, etc. —
  // anything the bundled examples actually use. The check is "an
  // identifier that looks like a call but isn't whitelisted".
  const KNOWN = new Set([
    "log",
    "print",
    "println",
    "Member",
    "Panel",
    "Suite",
    "Case",
    "Compare",
    "Browser",
    "main",
    "greet",
    "fn",
    "let",
    "if",
    "else",
    "match",
    "for",
    "while",
    "return",
    "use",
    "package",
    "struct",
    "enum",
    "type",
    "agent",
    "protocol",
    "on",
    "std",
    "Result",
    "Ok",
    "Err",
    "Some",
    "None",
    "true",
    "false",
    "self",
    "Self",
  ]);
  const callRx = /\b([a-z_][a-z0-9_]*)\s*\(/g;
  let m: RegExpExecArray | null;
  let unknownReported = 0;
  while ((m = callRx.exec(source)) !== null) {
    const name = m[1];
    if (KNOWN.has(name)) continue;
    // Skip method calls (preceded by `.`).
    if (m.index > 0 && source[m.index - 1] === ".") continue;
    // Skip our own example fns: prefixed with `_`.
    if (name.startsWith("_")) continue;
    // Skip if it has a `fn <name>` definition somewhere in the source.
    if (new RegExp(`\\bfn\\s+${name}\\b`).test(source)) continue;
    const span = spanFromOffset(source, m.index, m.index + name.length);
    diags.push({
      code: "MT0301",
      severity: "warning",
      message: `unresolved name \`${name}\``,
      primary: { span, message: "not found in scope" },
      secondary: [],
      notes: [],
      helps: [
        "this is a mock-backend warning — the real compiler may resolve it via macro expansion or stdlib re-exports",
      ],
      fixes: [],
    });
    unknownReported++;
    if (unknownReported >= 3) break;
  }

  return diags;
}

/** Produce stdout from a source by extracting every literal string
 *  argument to a `log(...)` call. Good enough to make Hello World
 *  feel real. */
function mockStdout(source: string): string {
  const out: string[] = [];
  const rx = /\blog\s*\(\s*"((?:[^"\\]|\\.)*)"\s*\)/g;
  let m: RegExpExecArray | null;
  while ((m = rx.exec(source)) !== null) {
    out.push(unescapeMtyString(m[1]));
  }
  return out.length > 0
    ? out.join("\n") + "\n"
    : "// program ran. no `log(...)` calls produced output.\n";
}

function unescapeMtyString(s: string): string {
  return s.replace(/\\n/g, "\n").replace(/\\t/g, "\t").replace(/\\"/g, '"');
}

class MockRunner implements Runner {
  readonly mode = "mock" as const;
  async init() {}
  async check(source: string): Promise<CheckResult> {
    await sleep(80);
    const diagnostics = mockDiagnostics(source);
    const ok = !diagnostics.some(severityIsError);
    return { ok, diagnostics };
  }
  async run(source: string): Promise<RunResult> {
    await sleep(120);
    const diagnostics = mockDiagnostics(source);
    const fatal = diagnostics.some(severityIsError);
    if (fatal) {
      return {
        ok: false,
        stdout: "",
        diagnostics,
        trace: "// program rejected before run — see diagnostics",
      };
    }
    return {
      ok: true,
      stdout: mockStdout(source),
      diagnostics,
      trace: mockTrace(source),
    };
  }
}

function mockTrace(source: string): string {
  const fnCount = (source.match(/\bfn\s+\w+/g) || []).length;
  const useCount = (source.match(/\buse\s+[\w.]+/g) || []).length;
  return [
    "[mock-trace] parse: 1 file",
    `[mock-trace] hir.lower: ${fnCount} fn(s), ${useCount} use(s)`,
    "[mock-trace] types.check: clean",
    "[mock-trace] borrow.check: clean",
    "[mock-trace] sir.lower: ok",
    "[mock-trace] interp.run: exit 0",
    "",
    "// real trace lands when crates/mty-cli wasm target ships (see README)",
  ].join("\n");
}

function severityIsError(d: Diagnostic): boolean {
  return d.severity === "error";
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

// ---------------------------------------------------------------------------
// Wasm runner (stub — wired up but not loaded until the artifact exists)
// ---------------------------------------------------------------------------

class WasmRunner implements Runner {
  readonly mode = "wasm" as const;
  private mod: WasmModule | null = null;

  async init() {
    // The wasm-pack `--target web` shim is an ES module that default-
    // exports an init function. We `import(...)` it dynamically so the
    // mock build doesn't have to ship the .js.
    const url = new URL("../public/mty_playground.js", import.meta.url);
    const mod = (await import(/* @vite-ignore */ url.href)) as WasmModule;
    await mod.default();
    this.mod = mod;
  }

  async check(source: string): Promise<CheckResult> {
    if (!this.mod) throw new Error("wasm runner not initialised");
    const raw = this.mod.check(source) as RawDiagPayload;
    return {
      ok: raw.ok,
      diagnostics: hydrateRawDiags(raw.diagnostics ?? [], source),
    };
  }

  async run(source: string): Promise<RunResult> {
    if (!this.mod) throw new Error("wasm runner not initialised");
    const raw = this.mod.run(source) as RawRunPayload;
    return {
      ok: raw.ok,
      stdout: raw.stdout ?? "",
      diagnostics: hydrateRawDiags(raw.diagnostics ?? [], source),
      trace: raw.trace ?? "",
    };
  }
}

// ---------------------------------------------------------------------------
// Raw payload shapes from the wasm side. T4 ratifies these.
// ---------------------------------------------------------------------------

interface RawDiag {
  code: string;
  severity: Severity;
  message: string;
  primary: { start: number; end: number; message: string };
  secondary?: { start: number; end: number; message: string }[];
  notes?: string[];
  helps?: string[];
  fixes?: {
    title: string;
    start?: number;
    end?: number;
    replacement?: string;
  }[];
}

interface RawDiagPayload {
  ok: boolean;
  diagnostics?: RawDiag[];
}

interface RawRunPayload extends RawDiagPayload {
  stdout?: string;
  trace?: string;
}

interface WasmModule {
  default: () => Promise<unknown>;
  check: (src: string) => unknown;
  run: (src: string) => unknown;
}

function hydrateRawDiags(raw: RawDiag[], source: string): Diagnostic[] {
  return raw.map((d) => ({
    code: d.code,
    severity: d.severity,
    message: d.message,
    primary: {
      span: spanFromOffset(source, d.primary.start, d.primary.end),
      message: d.primary.message,
    },
    secondary: (d.secondary ?? []).map((s) => ({
      span: spanFromOffset(source, s.start, s.end),
      message: s.message,
    })),
    notes: d.notes ?? [],
    helps: d.helps ?? [],
    fixes: (d.fixes ?? []).map((f) => ({
      title: f.title,
      span:
        f.start !== undefined && f.end !== undefined
          ? spanFromOffset(source, f.start, f.end)
          : undefined,
      replacement: f.replacement,
    })),
  }));
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

// `USE_WASM_BACKEND` is a Vite-time `define` from vite.config.ts.
// Defaults to `false` so a stock `npm run build` ships a working
// mock-backend playground. The ambient `ImportMetaEnv` interface is
// declared in `src/vite-env.d.ts`.
export function makeRunner(): Runner {
  if (import.meta.env.USE_WASM_BACKEND) {
    return new WasmRunner();
  }
  return new MockRunner();
}
