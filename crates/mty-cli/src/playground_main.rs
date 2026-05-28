// v0.33 T3 — browser-playground entry point for `tools/playground/`.
//
// This is the wasm-bindgen front door for the in-browser Mighty
// compiler. It exposes three JS-side calls:
//
//   init()                   — Boot the wasm module. wasm-bindgen
//                              calls this for us via `--target web`.
//   check(src: &str) -> JS   — Parse + HIR-lower + type-check + borrow-check.
//                              Returns { ok, diagnostics[] }.
//   run(src: &str)   -> JS   — `check` + SIR lower + tree-walker
//                              interpret. Returns { ok, stdout, trace,
//                              diagnostics[] }.
//
// Diagnostic envelope shape (subject to T4 ratification — see
// tools/playground/src/runner.ts):
//
//   {
//     "code": "MT4099",
//     "severity": "error" | "warning" | "note" | "help",
//     "message": "tainted value flows to ...",
//     "primary":  { "start": 312, "end": 322, "message": "..." },
//     "secondary": [{ "start": ..., "end": ..., "message": "..." }, ...],
//     "notes":  ["tainted values may not reach a sink", ...],
//     "helps":  ["untaint via .matches_regex(...) ...", ...],
//     "fixes":  [{ "title": "...", "start": ..., "end": ..., "replacement": "..." }, ...]
//   }
//
// Build (gated behind `--features playground-wasm`; see Cargo.toml):
//
//   wasm-pack build --target web \
//     --out-dir ../../tools/playground/public \
//     --out-name mty_playground \
//     --no-default-features \
//     --features playground-wasm \
//     crates/mty-cli
//
// v0.33 ships:
//   - this entry point
//   - the Cargo.toml stanza + feature gate
//   - the playground UI working against the mock backend
//
// v0.34 follow-up — what's needed to actually emit the wasm artifact:
//   - Split host-only deps (`mty-codegen-cranelift`, `mty-codegen-llvm`,
//     `mty-codegen-wasm`, `mty-runtime` full, `tokio`, `hyper`,
//     `notify`, `rusqlite` via `observe-sqlite`) behind a
//     `host-toolchain` Cargo feature so wasm32 builds don't transitively
//     drag them in.
//   - Either extract this entry point into its own crate
//     (`crates/mty-playground`) with `[lib] crate-type = ["cdylib"]`,
//     OR add `cdylib` to mty-cli's `[lib]` and let wasm-pack target
//     the library rather than this bin.
// Both moves are mechanical; the surface contract here doesn't change.

#![cfg_attr(
    not(target_arch = "wasm32"),
    allow(dead_code, unused_imports)
)]

#[cfg(all(target_arch = "wasm32", feature = "playground-wasm"))]
mod wasm {
    use mty_diagnostics::{Diagnostic, Severity};
    use mty_driver::{lower, parse_source, type_and_borrow_check};
    use serde::Serialize;
    use wasm_bindgen::prelude::*;

    // ---- JSON envelope shapes ------------------------------------------

    #[derive(Serialize)]
    struct DiagOut<'a> {
        code: String,
        severity: &'static str,
        message: String,
        primary: LabelOut<'a>,
        secondary: Vec<LabelOut<'a>>,
        notes: Vec<&'a str>,
        helps: Vec<&'a str>,
        fixes: Vec<FixOut>,
    }

    #[derive(Serialize)]
    struct LabelOut<'a> {
        start: usize,
        end: usize,
        message: &'a str,
    }

    #[derive(Serialize)]
    struct FixOut {
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        start: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        end: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        replacement: Option<String>,
    }

    #[derive(Serialize)]
    struct CheckOut<'a> {
        ok: bool,
        diagnostics: Vec<DiagOut<'a>>,
    }

    #[derive(Serialize)]
    struct RunOut<'a> {
        ok: bool,
        stdout: String,
        trace: String,
        diagnostics: Vec<DiagOut<'a>>,
    }

    fn severity_str(s: Severity) -> &'static str {
        match s {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
            Severity::Help => "help",
        }
    }

    fn to_out<'a>(d: &'a Diagnostic) -> DiagOut<'a> {
        DiagOut {
            code: format!("{}", d.code),
            severity: severity_str(d.severity),
            message: d.primary.message.clone(),
            primary: LabelOut {
                start: d.primary.start,
                end: d.primary.end,
                message: &d.primary.message,
            },
            secondary: d
                .secondary
                .iter()
                .map(|l| LabelOut {
                    start: l.start,
                    end: l.end,
                    message: &l.message,
                })
                .collect(),
            notes: d.notes.iter().map(|s| s.as_str()).collect(),
            helps: d.helps.iter().map(|s| s.as_str()).collect(),
            // v0.33 T3 — the in-tree Diagnostic struct doesn't yet carry
            // structured fix envelopes; T4 adds the `fixes` field and
            // this initializer becomes `d.fixes.iter().map(...)`.
            fixes: Vec::new(),
        }
    }

    fn any_error(diags: &[Diagnostic]) -> bool {
        diags
            .iter()
            .any(|d| matches!(d.severity, Severity::Error))
    }

    // ---- Exported surface ----------------------------------------------

    /// One-time module init. wasm-pack invokes this via `default()` on
    /// the JS side. Right now it's a no-op; v0.34 wires up `console_log`
    /// + `console_error` panic hooks here.
    #[wasm_bindgen]
    pub fn init() {
        // wasm-bindgen-future-proof slot.
    }

    /// Parse + HIR-lower + type+borrow check. Returns a JSON envelope
    /// `{ ok, diagnostics[] }`.
    #[wasm_bindgen]
    pub fn check(src: &str) -> JsValue {
        let diags = check_inner(src);
        let ok = !any_error(&diags);
        let out = CheckOut {
            ok,
            diagnostics: diags.iter().map(to_out).collect(),
        };
        serde_wasm_bindgen::to_value(&out)
            .unwrap_or(JsValue::NULL)
    }

    /// `check` + SIR lower + tree-walker interpret. Returns
    /// `{ ok, stdout, trace, diagnostics[] }`.
    #[wasm_bindgen]
    pub fn run(src: &str) -> JsValue {
        let diags = check_inner(src);
        if any_error(&diags) {
            let out = RunOut {
                ok: false,
                stdout: String::new(),
                trace: String::from("// rejected before run — see diagnostics"),
                diagnostics: diags.iter().map(to_out).collect(),
            };
            return serde_wasm_bindgen::to_value(&out)
                .unwrap_or(JsValue::NULL);
        }
        // v0.33 ships the diagnostics path; the interpreter-on-wasm path
        // lands once mty-runtime's tokio-free `BudgetTracker`/`StdHost`
        // are split out behind a `host-toolchain` Cargo feature. Until
        // then, return a clean "no-output" run so the UI behaves.
        let out = RunOut {
            ok: true,
            stdout: String::from("// (wasm interpreter wiring pending — see v0.34 follow-up)\n"),
            trace: String::from(
                "[wasm-trace] parse: ok\n\
                 [wasm-trace] hir.lower: ok\n\
                 [wasm-trace] types.check: ok\n\
                 [wasm-trace] borrow.check: ok\n\
                 [wasm-trace] interp: pending (v0.34)\n",
            ),
            diagnostics: diags.iter().map(to_out).collect(),
        };
        serde_wasm_bindgen::to_value(&out).unwrap_or(JsValue::NULL)
    }

    fn check_inner(src: &str) -> Vec<Diagnostic> {
        let parsed = parse_source(src.to_string(), "playground.mty".to_string());
        let (pkg, mut diags) = lower(&parsed);
        if !any_error(&diags) {
            diags.extend(type_and_borrow_check(&pkg));
        }
        diags
    }
}

// ---- Native stub --------------------------------------------------------
//
// The [[bin]] target needs a fn main() to satisfy the linker on the
// host. We never actually invoke the playground binary natively — the
// build is gated behind `required-features = ["playground-wasm"]` and
// the feature is wasm-only — but Cargo still wants a main fn at parse
// time. This shim does nothing and is unreachable in practice.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!(
        "mty-playground is a wasm-bindgen entry point — see \
         tools/playground/README.md for the wasm-pack build flow."
    );
}

#[cfg(target_arch = "wasm32")]
fn main() {}
