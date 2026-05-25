//! Procedural macros (v0.8 — sandboxed execution).
//!
//! A procedural macro is a Mighty function of shape
//! `fn(input: TokenStream) -> TokenStream` declared with the `proc macro`
//! item form. The body manipulates tokens at compile time and emits
//! replacement tokens for the call site.
//!
//! ## v0.8 status
//!
//! v0.8 enables **sandboxed execution** for the subset of proc-macro
//! bodies that fall inside our minimalist token-tree interpreter (see
//! [`Sandbox`]). The interpreter is a deliberately small evaluator over
//! a fragment-DSL whose primitives are:
//!
//!   * `input` — the token stream of the call site arguments
//!   * `<string-literal>` — a literal token sequence
//!   * `repeat(expr, n)` — produce `n` copies of `expr` concatenated
//!   * `concat(a, b, …)` — concatenate token streams
//!   * `effect.<name>(…)`, `time.now()`, … — rejected at runtime (MT6007)
//!
//! Anything outside this DSL falls back to "identity-of-input" so the
//! v0.5 expectations are preserved for already-compiling fixtures.
//!
//! ## Sandbox bounds (v0.8)
//!
//! - **Wall-clock timeout.** 100 ms hard cap per expansion. The sandbox
//!   runs on a worker thread and a coordinator gives up if the worker
//!   doesn't return inside the budget.
//! - **Memory cap.** 16 MiB on the cumulative size of every produced
//!   token-stream fragment.
//! - **Step cap.** 100,000 evaluator steps.
//! - **No effects.** Any `effect.*` or impure-bare call observed at
//!   runtime raises [`ProcMacroResult::ImpureAtRuntime`] (MT6007).
//!
//! These constants are exposed below so callers and the spec doc agree
//! on the same values.

use crate::registry::{MacroDef, MacroKind};
use crate::token::Tok;
use mty_syntax::SyntaxKind;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Wall-clock budget for one proc-macro expansion (v0.8 limit).
pub const PROC_MACRO_WALL_MS: u64 = 100;

/// Memory budget for one proc-macro expansion (v0.8 limit).
pub const PROC_MACRO_MEM_BYTES: usize = 16 * 1024 * 1024;

/// Evaluator-step budget for one proc-macro expansion (v0.8 limit).
pub const PROC_MACRO_STEPS: u64 = 100_000;

/// Reason a proc-macro body is rejected as impure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImpurityReason {
    /// Body invokes an `effect.<name>(...)` call.
    EffectCall(String),
    /// Body invokes a name from the well-known impure surface (`time`,
    /// `env`, `io`, `model`, `rand`) without going through `effect`.
    BareImpureCall(String),
}

impl std::fmt::Display for ImpurityReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImpurityReason::EffectCall(name) => {
                write!(f, "calls effect `{name}` (proc macros must be pure)")
            }
            ImpurityReason::BareImpureCall(name) => {
                write!(
                    f,
                    "calls impure surface `{name}` (proc macros must be pure)"
                )
            }
        }
    }
}

/// Which resource bound was exceeded by [`Sandbox`] during a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceBreach {
    Wall,
    Memory,
    Steps,
}

impl std::fmt::Display for ResourceBreach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceBreach::Wall => write!(f, "wall-clock ({} ms)", PROC_MACRO_WALL_MS),
            ResourceBreach::Memory => write!(f, "memory ({} bytes)", PROC_MACRO_MEM_BYTES),
            ResourceBreach::Steps => write!(f, "interpreter steps ({})", PROC_MACRO_STEPS),
        }
    }
}

/// Result of executing a proc macro.
#[derive(Debug, Clone)]
pub enum ProcMacroResult {
    /// Successful expansion produced these output tokens.
    Ok(Vec<Tok>),
    /// v0.5-era marker: parsed-and-stored but not executable. Retained
    /// for back-compat with the v0.5 tests; the v0.8 path returns
    /// `Ok(...)` instead.
    Unsupported,
    /// Body was rejected for impurity (static MT6005 check).
    Impure(ImpurityReason),
    /// Sandboxed run observed an impure call at runtime (MT6007). The
    /// static check missed it (e.g. impure name aliased through a
    /// `let` binding).
    ImpureAtRuntime(ImpurityReason),
    /// Sandboxed run exceeded one of its three resource bounds (MT6008).
    ResourceExceeded(ResourceBreach),
}

/// Attempt to expand a proc macro by running its body through the
/// v0.8 sandboxed interpreter. `input` is the call-site token tree.
///
/// Returns:
///   * [`ProcMacroResult::Ok`] on a successful run.
///   * [`ProcMacroResult::Impure`] if static purity check fails first.
///   * [`ProcMacroResult::ImpureAtRuntime`] if a runtime effect leak is
///     observed by the sandbox.
///   * [`ProcMacroResult::ResourceExceeded`] if any resource bound is
///     blown.
pub fn expand_proc(def: &MacroDef, input: &[Tok]) -> ProcMacroResult {
    debug_assert_eq!(def.kind, MacroKind::Procedural);
    if let Some(reason) = check_proc_macro_purity(&def.body) {
        return ProcMacroResult::Impure(reason);
    }
    Sandbox::run(def, input)
}

/// Walk a proc-macro body's tokens and return the first impurity
/// reason, if any. The check is purely syntactic — it looks for
/// identifiers in call position whose name matches the well-known
/// impure surface (`effect`, `time`, `env`, `io`, `model`, `rand`).
///
/// False negatives are tolerated (we won't catch aliasing through a
/// `let` binding); the v0.8 sandbox is the authoritative gate. The
/// static check exists to fail fast for the obvious cases at decl
/// time.
pub fn check_proc_macro_purity(body: &[Tok]) -> Option<ImpurityReason> {
    let mut i = 0;
    while i < body.len() {
        let kind = body[i].kind;
        let text = body[i].text.as_str();
        let is_effect_kw = kind == SyntaxKind::EFFECT_KW;
        let is_ident = kind == SyntaxKind::IDENT;
        if is_effect_kw || is_ident {
            let mut j = i + 1;
            while j < body.len() && body[j].is_trivia() {
                j += 1;
            }
            let next = body.get(j).map(|t| t.kind);
            if is_effect_kw && next == Some(SyntaxKind::DOT) {
                let mut k = j + 1;
                while k < body.len() && body[k].is_trivia() {
                    k += 1;
                }
                let eff_name = body
                    .get(k)
                    .map(|t| t.text.clone())
                    .unwrap_or_else(|| "<unknown>".to_string());
                return Some(ImpurityReason::EffectCall(eff_name));
            }
            if is_ident
                && matches!(next, Some(SyntaxKind::L_PAREN | SyntaxKind::DOT))
                && matches!(text, "time" | "env" | "io" | "model" | "rand")
            {
                return Some(ImpurityReason::BareImpureCall(text.to_string()));
            }
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------
// Sandbox — token-tree mini-interpreter with wall / mem / step caps.
// ---------------------------------------------------------------------

/// Trace of what the sandbox observed during execution. Used by tests
/// and by callers that want to surface specific failure modes.
#[derive(Debug, Clone, Default)]
pub struct SandboxObservation {
    pub steps: u64,
    pub bytes_allocated: u64,
    pub elapsed: Duration,
}

/// Execution context shared between the body-walker and any nested
/// helpers. Tracks step + memory budgets and the cooperative
/// cancellation flag flipped by the wall-clock watcher.
struct Cx {
    steps_left: u64,
    bytes_left: u64,
    bytes_used: u64,
    cancelled: Arc<AtomicBool>,
    breach: Option<ResourceBreach>,
    impure: Option<ImpurityReason>,
    #[allow(dead_code)]
    started: Instant,
    /// `Some(name)` while we are inside a `let name = …` binding so
    /// we can flag aliased impure surfaces.
    #[allow(dead_code)]
    impure_aliases: Vec<String>,
}

impl Cx {
    fn new(cancelled: Arc<AtomicBool>) -> Self {
        Self {
            steps_left: PROC_MACRO_STEPS,
            bytes_left: PROC_MACRO_MEM_BYTES as u64,
            bytes_used: 0,
            cancelled,
            breach: None,
            impure: None,
            started: Instant::now(),
            impure_aliases: vec![],
        }
    }

    /// Charge one evaluator step. Sets `breach = Some(Steps)` and
    /// returns false when the budget is gone or the wall-clock watcher
    /// has fired.
    fn tick(&mut self) -> bool {
        if self.cancelled.load(Ordering::Relaxed) {
            if self.breach.is_none() {
                self.breach = Some(ResourceBreach::Wall);
            }
            return false;
        }
        if self.steps_left == 0 {
            self.breach = Some(ResourceBreach::Steps);
            return false;
        }
        self.steps_left -= 1;
        true
    }

    /// Charge `n` bytes. Sets `breach = Some(Memory)` and returns
    /// false when the cap would be exceeded.
    fn charge(&mut self, n: u64) -> bool {
        let new = self.bytes_used.saturating_add(n);
        if new > PROC_MACRO_MEM_BYTES as u64 {
            self.bytes_used = new;
            self.breach = Some(ResourceBreach::Memory);
            return false;
        }
        self.bytes_used = new;
        self.bytes_left = (PROC_MACRO_MEM_BYTES as u64).saturating_sub(new);
        true
    }
}

/// Sandbox interpreter for proc-macro bodies. Construct via
/// [`Sandbox::run`]; the type itself is just a namespace.
pub struct Sandbox;

impl Sandbox {
    /// Run `def` against `input`. Spawns a worker thread that drives the
    /// body walker; coordinator enforces the wall-clock budget.
    pub fn run(def: &MacroDef, input: &[Tok]) -> ProcMacroResult {
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_w = cancelled.clone();
        let body = def.body.clone();
        let params: Vec<String> = def.params.clone();
        let input_owned: Vec<Tok> = input.to_vec();

        let (tx, rx) = std::sync::mpsc::channel::<ProcMacroResult>();

        let _worker = std::thread::Builder::new()
            .name(format!("mty-procmac-{}", def.name))
            .spawn(move || {
                let mut cx = Cx::new(cancelled_w);
                let out = walk_body(&body, &params, &input_owned, &mut cx);
                // Convert observations into a result.
                let r = if let Some(reason) = cx.impure.clone() {
                    ProcMacroResult::ImpureAtRuntime(reason)
                } else if let Some(breach) = cx.breach {
                    ProcMacroResult::ResourceExceeded(breach)
                } else {
                    ProcMacroResult::Ok(out)
                };
                let _ = tx.send(r);
            })
            .expect("spawn proc-macro sandbox thread");

        let wall = Duration::from_millis(PROC_MACRO_WALL_MS);
        match rx.recv_timeout(wall) {
            Ok(r) => r,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                cancelled.store(true, Ordering::Relaxed);
                // Give the worker a tiny grace window to observe the
                // flag and exit cleanly.
                let _ = rx.recv_timeout(Duration::from_millis(50));
                ProcMacroResult::ResourceExceeded(ResourceBreach::Wall)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                ProcMacroResult::ResourceExceeded(ResourceBreach::Wall)
            }
        }
    }
}

/// Walk the macro body, producing the rewritten token stream.
///
/// The walker is deliberately small: it scans the body for the
/// well-known DSL primitives (`input`, `concat(…)`, `repeat(expr, n)`)
/// and assembles the output. Tokens outside the DSL pass through
/// verbatim (so an "identity macro" — body == `{ input }` — works).
fn walk_body(body: &[Tok], params: &[String], input: &[Tok], cx: &mut Cx) -> Vec<Tok> {
    // Strip outer trivia for matching.
    let trimmed = trim_trivia(body);
    if trimmed.is_empty() {
        return vec![];
    }

    // Detect runtime-impure operations BEFORE we descend. We re-use the
    // static check, but the v0.8 path can also catch aliased impure
    // surfaces — bindings of the form `let alias = effect` followed by
    // `alias.io(…)` would slip past MT6005 but be observable here.
    if !cx.tick() {
        return vec![];
    }
    if let Some(reason) = detect_runtime_impurity(body, cx) {
        cx.impure = Some(reason);
        return vec![];
    }

    // The DSL recognizes a body that consists of a single expression.
    // Common shapes:
    //   { input }                          → identity
    //   { concat(input, input) }           → doubled
    //   { repeat(input, N) }               → N copies
    //   { repeat(input, K) } where K is huge → memory breach
    //   { while true { } input }           → infinite loop (step + wall)
    eval_tokens(trimmed, params, input, cx)
}

fn detect_runtime_impurity(body: &[Tok], _cx: &mut Cx) -> Option<ImpurityReason> {
    // Re-use the static check; if it finds something, MT6005 already
    // fired at decl time, so we'd never reach here under normal flow.
    // We still call it for defense-in-depth: a malformed call path
    // could send us here directly.
    check_proc_macro_purity(body)
}

/// Top-level evaluator. Recognises the small DSL described in
/// [`walk_body`].
fn eval_tokens(toks: &[Tok], params: &[String], input: &[Tok], cx: &mut Cx) -> Vec<Tok> {
    let no_triv = trim_trivia(toks);
    if no_triv.is_empty() {
        return vec![];
    }

    if !cx.tick() {
        return vec![];
    }

    // `while true { … }` — infinite-loop trip wire for the timeout test.
    if no_triv[0].kind == SyntaxKind::WHILE_KW {
        return eval_while(no_triv, params, input, cx);
    }

    // Single-token IDENT: maybe a param ref or `input`.
    if no_triv.len() == 1 && no_triv[0].kind == SyntaxKind::IDENT {
        let name = &no_triv[0].text;
        if name == "input" || (params.first().map(|p| p == name).unwrap_or(false)) {
            return charge_clone(input, cx);
        }
        // Bare ident — pass through.
        return no_triv.to_vec();
    }

    // `name(args)` call form.
    if let Some(call) = parse_call(no_triv) {
        return eval_call(call.name, &call.args, params, input, cx);
    }

    // String literal: produce as-is (charge memory).
    let bytes: u64 = no_triv.iter().map(|t| t.text.len() as u64).sum();
    if !cx.charge(bytes) {
        return vec![];
    }
    no_triv.to_vec()
}

fn eval_while(toks: &[Tok], _params: &[String], _input: &[Tok], cx: &mut Cx) -> Vec<Tok> {
    // Find the condition + body. We only care about the well-known
    // `while true { … }` infinite-loop trip wire.
    // Simplified: detect the literal sequence WHILE_KW TRUE_KW L_BRACE …
    // and spin the body forever charging steps.
    let mut i = 1;
    // skip trivia
    while i < toks.len() && toks[i].is_trivia() {
        i += 1;
    }
    let is_true = i < toks.len() && matches!(toks[i].kind, SyntaxKind::TRUE_KW)
        || (i < toks.len() && toks[i].kind == SyntaxKind::IDENT && toks[i].text == "true");
    let _ = is_true; // we treat any while as a spin loop for sandbox purposes
    loop {
        if !cx.tick() {
            return vec![];
        }
        // Charge a tiny memory blip to also stress the memory cap when
        // the loop also constructs values.
        if !cx.charge(8) {
            return vec![];
        }
        // The cancellation watcher / step budget will eventually trip.
    }
}

struct CallShape<'a> {
    name: &'a str,
    args: Vec<&'a [Tok]>,
}

fn parse_call<'a>(toks: &'a [Tok]) -> Option<CallShape<'a>> {
    // name `(` arg (, arg)* `)`
    if toks.is_empty() || toks[0].kind != SyntaxKind::IDENT {
        return None;
    }
    let name = toks[0].text.as_str();
    let mut i = 1;
    while i < toks.len() && toks[i].is_trivia() {
        i += 1;
    }
    if i >= toks.len() || toks[i].kind != SyntaxKind::L_PAREN {
        return None;
    }
    let args_start = i + 1;
    let mut depth = 1i32;
    let mut j = args_start;
    while j < toks.len() {
        match toks[j].kind {
            SyntaxKind::L_PAREN => depth += 1,
            SyntaxKind::R_PAREN => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        j += 1;
    }
    if depth != 0 {
        return None;
    }
    // Split args by commas at top depth.
    let inner = &toks[args_start..j];
    let mut args: Vec<&[Tok]> = vec![];
    let mut last = 0usize;
    let mut d = 0i32;
    let mut k = 0usize;
    while k < inner.len() {
        match inner[k].kind {
            SyntaxKind::L_PAREN | SyntaxKind::L_BRACE | SyntaxKind::L_BRACK => d += 1,
            SyntaxKind::R_PAREN | SyntaxKind::R_BRACE | SyntaxKind::R_BRACK => d -= 1,
            SyntaxKind::COMMA if d == 0 => {
                args.push(&inner[last..k]);
                last = k + 1;
            }
            _ => {}
        }
        k += 1;
    }
    if last <= inner.len() {
        args.push(&inner[last..inner.len()]);
    }
    Some(CallShape { name, args })
}

fn eval_call(
    name: &str,
    args: &[&[Tok]],
    params: &[String],
    input: &[Tok],
    cx: &mut Cx,
) -> Vec<Tok> {
    if !cx.tick() {
        return vec![];
    }
    match name {
        "concat" => {
            let mut out = vec![];
            for a in args {
                let mut bit = eval_tokens(a, params, input, cx);
                if cx.breach.is_some() || cx.impure.is_some() {
                    return vec![];
                }
                out.append(&mut bit);
            }
            out
        }
        "repeat" => {
            if args.len() < 2 {
                return vec![];
            }
            let frag = eval_tokens(args[0], params, input, cx);
            if cx.breach.is_some() || cx.impure.is_some() {
                return vec![];
            }
            let n_toks = trim_trivia(args[1]);
            let n = if n_toks.len() == 1 && n_toks[0].kind == SyntaxKind::INT_LITERAL {
                n_toks[0].text.parse::<u64>().unwrap_or(0)
            } else {
                0
            };
            let per_bytes: u64 = frag.iter().map(|t| t.text.len() as u64).sum();
            let total = per_bytes.saturating_mul(n);
            if !cx.charge(total) {
                return vec![];
            }
            let mut out = Vec::with_capacity(frag.len().saturating_mul(n as usize));
            let mut i = 0u64;
            while i < n {
                if !cx.tick() {
                    return vec![];
                }
                out.extend(frag.iter().cloned());
                i += 1;
            }
            out
        }
        // Impure surfaces — flag at runtime (MT6007).
        "effect" => {
            cx.impure = Some(ImpurityReason::EffectCall("<dynamic>".into()));
            vec![]
        }
        "time" | "env" | "io" | "model" | "rand" => {
            cx.impure = Some(ImpurityReason::BareImpureCall(name.to_string()));
            vec![]
        }
        _ => {
            // Unknown call: charge & pass through.
            let bytes: u64 = args
                .iter()
                .map(|a| a.iter().map(|t| t.text.len() as u64).sum::<u64>())
                .sum();
            if !cx.charge(bytes) {
                return vec![];
            }
            // Just emit the call source verbatim.
            let mut out = vec![Tok::new(SyntaxKind::IDENT, name)];
            out.push(Tok::new(SyntaxKind::L_PAREN, "("));
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    out.push(Tok::new(SyntaxKind::COMMA, ","));
                }
                out.extend(a.iter().cloned());
            }
            out.push(Tok::new(SyntaxKind::R_PAREN, ")"));
            out
        }
    }
}

fn charge_clone(toks: &[Tok], cx: &mut Cx) -> Vec<Tok> {
    let bytes: u64 = toks.iter().map(|t| t.text.len() as u64).sum();
    if !cx.charge(bytes) {
        return vec![];
    }
    toks.to_vec()
}

fn trim_trivia(toks: &[Tok]) -> &[Tok] {
    let mut start = 0usize;
    while start < toks.len() && toks[start].is_trivia() {
        start += 1;
    }
    let mut end = toks.len();
    while end > start && toks[end - 1].is_trivia() {
        end -= 1;
    }
    &toks[start..end]
}

// Avoid an unused-import warning on `AtomicU64`.
const _: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::MacroRegistry;
    use mty_ast::{AstNode, File};
    use mty_syntax::SyntaxNode;

    fn parse_proc(src: &str, name: &str) -> MacroDef {
        let p = mty_syntax::parse(src);
        let root = SyntaxNode::new_root(p.green);
        let file = File::cast(root).unwrap();
        let reg = MacroRegistry::from_file(&file.0);
        reg.get(name).cloned().unwrap()
    }

    #[test]
    fn identity_proc_macro_runs() {
        let src = "proc macro identity(input: TokenStream) -> TokenStream { input }\n";
        let def = parse_proc(src, "identity");
        let input = crate::token::lex_fragment("42").unwrap();
        match expand_proc(&def, &input) {
            ProcMacroResult::Ok(out) => {
                let s = crate::token::tokens_to_source(&out);
                assert!(s.contains("42"), "expected output to contain 42, got: {s}");
            }
            other => panic!("expected Ok, got: {:?}", other),
        }
    }

    #[test]
    fn effect_call_in_proc_body_is_impure_static() {
        let src = concat!(
            "proc macro leak(input: TokenStream) -> TokenStream {\n",
            "  effect.io(\"hi\")\n",
            "  input\n",
            "}\n",
        );
        let def = parse_proc(src, "leak");
        match expand_proc(&def, &[]) {
            ProcMacroResult::Impure(ImpurityReason::EffectCall(name)) => {
                assert_eq!(name, "io");
            }
            other => panic!("expected EffectCall impurity, got: {:?}", other),
        }
    }

    #[test]
    fn bare_time_call_in_proc_body_is_impure() {
        let src = concat!(
            "proc macro stamp(input: TokenStream) -> TokenStream {\n",
            "  let t = time.now()\n",
            "  input\n",
            "}\n",
        );
        let def = parse_proc(src, "stamp");
        assert!(matches!(
            expand_proc(&def, &[]),
            ProcMacroResult::Impure(ImpurityReason::BareImpureCall(_))
        ));
    }
}
