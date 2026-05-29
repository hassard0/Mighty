// v0.33 T3 / v0.35 T1 — browser-playground entry point for
// `tools/playground/`.
//
// wasm-bindgen front door for the in-browser Mighty compiler. It
// exposes three JS-side calls:
//
//   init()                   — Boot the wasm module. wasm-pack wires
//                              this to the `default()` export emitted
//                              by `--target web`.
//   check(src: &str) -> JS   — Parse + HIR-lower + type-check + borrow-check.
//                              Returns { ok, diagnostics[] }.
//   run(src: &str)   -> JS   — `check` + SIR-lower + tree-walk interpret.
//                              Returns { ok, stdout, trace,
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
// Build (gated behind the `playground-wasm` feature; see Cargo.toml):
//
//   wasm-pack build --target web \
//     --no-default-features \
//     --features playground-wasm \
//     --out-dir ../../tools/playground/public/wasm \
//     crates/mty-cli
//
// v0.35 T1 ships this against the real backend. Previously it lived
// in `playground_main.rs` as a `[[bin]]` with a native shim main —
// that prevented wasm-pack from finding it (wasm-pack reads the lib
// cdylib). Moving to a lib module + dropping the bin lets the same
// `--no-default-features --features playground-wasm` invocation
// produce a real .wasm.

use mty_diagnostics::{Diagnostic, Severity};
use mty_driver::{lower, parse_source, type_and_borrow_check};
use serde::Serialize;
use wasm_bindgen::prelude::*;

// ---- JSON envelope shapes --------------------------------------------------

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

fn to_out(d: &Diagnostic) -> DiagOut<'_> {
    DiagOut {
        code: d.code.as_str(),
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
    diags.iter().any(|d| matches!(d.severity, Severity::Error))
}

// ---- Exported surface ------------------------------------------------------

/// One-time module init. wasm-pack invokes this via `default()` on the
/// JS side. Right now it's a no-op; once we add the
/// `console_error_panic_hook` crate (v0.36 follow-up) this hooks
/// Rust panics into the browser devtools instead of letting them
/// vanish into the wasm trap frame.
#[wasm_bindgen]
pub fn init() {
    // v0.36 follow-up: console_error_panic_hook::set_once();
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
    serde_wasm_bindgen::to_value(&out).unwrap_or(JsValue::NULL)
}

/// `check` + SIR-lower + tree-walk interpret. Returns
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
        return serde_wasm_bindgen::to_value(&out).unwrap_or(JsValue::NULL);
    }
    // v0.35 T1 — interpreter wiring. We run the tree-walk interpreter
    // (`mty_ir::interp::run`) with a captured stdout host so `log(...)`
    // calls render in the playground's stdout tab. The runtime + host
    // surface lives behind the host-toolchain feature, so we use a
    // minimal in-crate host shim instead.
    match run_inner(src) {
        Ok((stdout, trace)) => {
            let out = RunOut {
                ok: true,
                stdout,
                trace,
                diagnostics: diags.iter().map(to_out).collect(),
            };
            serde_wasm_bindgen::to_value(&out).unwrap_or(JsValue::NULL)
        }
        Err(msg) => {
            let out = RunOut {
                ok: false,
                stdout: String::new(),
                trace: format!("[wasm-trace] interp: trap: {msg}\n"),
                diagnostics: diags.iter().map(to_out).collect(),
            };
            serde_wasm_bindgen::to_value(&out).unwrap_or(JsValue::NULL)
        }
    }
}

fn check_inner(src: &str) -> Vec<Diagnostic> {
    let parsed = parse_source(src.to_string(), "playground.mty".to_string());
    let (pkg, mut diags) = lower(&parsed);
    if !any_error(&diags) {
        diags.extend(type_and_borrow_check(&pkg));
    }
    diags
}

// ---- Interpreter wiring ----------------------------------------------------

fn run_inner(src: &str) -> Result<(String, String), String> {
    use mty_ir::interp::host::BufferHost;
    use mty_ir::interp::run::run_fn_with_budget;
    use mty_ir::interp::RunResult;

    let parsed = parse_source(src.to_string(), "playground.mty".to_string());
    let (pkg, _) = lower(&parsed);
    let typed = mty_types::check_package_typed(&pkg);
    let prog = mty_ir::lower_package(&pkg, &typed);

    // `BufferHost` buffers stdout in memory and records every effect /
    // extern call. That's exactly the shape we need for the playground —
    // `log(...)` lands in `host.stdout`, std.* effect calls become
    // `Value::Unit` (silent, deterministic).
    let mut host = BufferHost::default();

    let trace_prefix = String::from(
        "[wasm-trace] parse: ok\n\
         [wasm-trace] hir.lower: ok\n\
         [wasm-trace] types.check: ok\n\
         [wasm-trace] borrow.check: ok\n\
         [wasm-trace] sir.lower: ok\n",
    );

    if prog.fn_by_name("main").is_none() {
        return Ok((
            host.stdout_str(),
            format!(
                "{trace_prefix}[wasm-trace] interp: no main, nothing to run (top-level statements are accepted but produce no output)\n"
            ),
        ));
    }

    match run_fn_with_budget(&prog, "main", vec![], &mut host, 1_000_000) {
        Ok(_) => Ok((
            host.stdout_str(),
            format!("{trace_prefix}[wasm-trace] interp.run: exit 0\n"),
        )),
        Err(RunResult::Trap { code, message }) => Err(format!("{code}: {message}")),
        Err(RunResult::BudgetExceeded) => Err("budget exceeded".into()),
        Err(RunResult::MemBudgetExceeded { used, limit }) => {
            Err(format!("memory budget exceeded: {used} B > {limit} B"))
        }
        Err(RunResult::NoMain) => Ok((
            host.stdout_str(),
            format!("{trace_prefix}[wasm-trace] interp: no main, nothing to run\n"),
        )),
        Err(RunResult::Ok { exit }) => Ok((
            host.stdout_str(),
            format!("{trace_prefix}[wasm-trace] interp.run: exit {exit}\n"),
        )),
    }
}
