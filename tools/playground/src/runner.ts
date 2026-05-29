// v0.33 T3 / v0.35 T1 — runner.
//
// Two backends behind one interface:
//
//   - WasmRunner:  dynamically imports the wasm-bindgen JS shim emitted
//                  by `wasm-pack build --target web --no-default-features
//                  --features playground-wasm crates/mty-cli` and calls
//                  into the playground exports defined in
//                  `crates/mty-cli/src/playground.rs`.
//
//   - MockRunner:  pattern-matches the source to produce plausible
//                  diagnostics + stdout. Kept as the offline fallback
//                  for `npm run dev` flows that don't have the wasm
//                  artifact built yet, and triggered automatically if
//                  the WASM module fails to load.
//
// The switch is `USE_WASM_BACKEND`, a Vite-time `define` (see
// vite.config.ts). v0.33 T3 shipped with this flipped to `false`
// (mock-backend only). v0.35 T1 flips it on for the GH-Pages build.

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
              'wrap `user_input` with `.sanitize_with(PathBoundary("./safe/"))`',
            span: primarySpan,
            replacement:
              'user_input.sanitize_with(PathBoundary("./safe/"))',
          },
          {
            title:
              'wrap `user_input` with `.matches_regex("^[a-z ]{1,80}$")`',
            span: primarySpan,
            replacement: 'user_input.matches_regex("^[a-z ]{1,80}$")',
          },
        ],
      });
    }
  }

  // ---- Unknown identifier (very common starter mistake) -------------------
  const KNOWN = new Set([
    "log", "print", "println", "Member", "Panel", "Suite", "Case",
    "Compare", "Browser", "main", "greet", "fn", "let", "if", "else",
    "match", "for", "while", "return", "use", "package", "struct",
    "enum", "type", "agent", "protocol", "on", "std", "Result", "Ok",
    "Err", "Some", "None", "true", "false", "self", "Self",
  ]);
  const callRx = /\b([a-z_][a-z0-9_]*)\s*\(/g;
  let m: RegExpExecArray | null;
  let unknownReported = 0;
  while ((m = callRx.exec(source)) !== null) {
    const name = m[1];
    if (KNOWN.has(name)) continue;
    if (m.index > 0 && source[m.index - 1] === ".") continue;
    if (name.startsWith("_")) continue;
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
 *  argument to a `log(...)` call. */
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
    await sleep(60);
    const diagnostics = mockDiagnostics(source);
    const ok = !diagnostics.some(severityIsError);
    return { ok, diagnostics };
  }
  async run(source: string): Promise<RunResult> {
    await sleep(80);
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
    "// using mock backend — set USE_WASM_BACKEND=true and build the wasm artifact for the real path",
  ].join("\n");
}

function severityIsError(d: Diagnostic): boolean {
  return d.severity === "error";
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

// ---------------------------------------------------------------------------
// Wasm runner
// ---------------------------------------------------------------------------

class WasmRunner implements Runner {
  readonly mode = "wasm" as const;
  private mod: WasmModule | null = null;

  async init() {
    // The wasm-pack `--target web` shim is an ES module that default-
    // exports an init function. We `import(...)` it dynamically so the
    // mock build doesn't have to ship the .js. `vite-ignore` tells Vite
    // to skip its rewrite (the wasm/* dir lives under `public/`, not
    // under `src/`, so Vite would otherwise try to resolve it at build
    // time and fail when the artifact isn't present in dev).
    //
    // The artifact name comes from wasm-pack: it derives the package /
    // module name from the Rust lib name (`mty_cli` in our case), so
    // the files we load are `mty_cli.js` + `mty_cli_bg.wasm`.
    const baseUrl = (import.meta as { env: { BASE_URL: string } }).env.BASE_URL || "./";
    const jsUrl = new URL(`wasm/mty_cli.js`, new URL(baseUrl, window.location.href));
    const mod = (await import(/* @vite-ignore */ jsUrl.href)) as WasmModule;
    // wasm-pack `--target web`'s default export wants the URL of the
    // .wasm next to it; we pass our own so the runtime fetch lands on
    // the same `wasm/` directory we just imported from.
    const wasmUrl = new URL(`wasm/mty_cli_bg.wasm`, new URL(baseUrl, window.location.href));
    await mod.default({ module_or_path: wasmUrl });
    mod.init();
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
  default: (init?: { module_or_path: URL }) => Promise<unknown>;
  init: () => void;
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
// v0.35 T1 flips the default to `true` (the wasm artifact ships with
// the playground). Falls back to the mock automatically if the WASM
// module fails to load (e.g. `npm run dev` with no wasm artifact built
// yet) — see `makeRunnerWithFallback`.
export function makeRunner(): Runner {
  if (import.meta.env.USE_WASM_BACKEND) {
    return new WasmRunner();
  }
  return new MockRunner();
}

/**
 * Try to initialise the WASM runner; on failure (typical in dev when
 * the wasm-pack artifact hasn't been built yet) fall back to the mock
 * runner. Returns the runner that successfully initialised.
 */
export async function makeRunnerWithFallback(): Promise<Runner> {
  if (!import.meta.env.USE_WASM_BACKEND) {
    const r = new MockRunner();
    await r.init();
    return r;
  }
  const wasm = new WasmRunner();
  try {
    await wasm.init();
    return wasm;
  } catch (e) {
    console.warn(
      "[playground] wasm runner failed to initialise — falling back to mock backend.",
      e,
    );
    const mock = new MockRunner();
    await mock.init();
    return mock;
  }
}
