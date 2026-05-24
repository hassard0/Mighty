//! Self-hosting bootstrap test (v0.4).
//!
//! Runs the Stardust lexer in `selfhost/lexer/lexer.sd` over a canned
//! input via the SIR interpreter, with a custom `Host` that services
//! the lexer's extern bridge (`lex_init` / `lex_len` / `lex_byte_at` /
//! `lex_slice` / `lex_emit`). Then it lexes the same input via the
//! trusted Rust lexer (`sdust_syntax::lex`) and diffs the two token
//! streams kind-by-kind + span-by-span.
//!
//! Bootstrap technique: see `docs/internals/self-hosting.md`.
//!
//! ## Why a hand-rolled host rather than `BufferHost`
//!
//! `BufferHost::extern_call` is permissive but ignorant — it returns
//! `Value::Unit` for everything. The lexer needs *real* byte access,
//! so this file installs a `SelfhostHost` that:
//!
//! * caches the source on `lex_init`
//! * returns the byte at offset `i` on `lex_byte_at` (or 256 for OOB,
//!   matching the lexer's sentinel)
//! * returns `src[start..end]` as a `Value::Str` on `lex_slice`
//! * records each emitted token in a `Vec<TokenRecord>` on `lex_emit`
//!
//! After the run, `host.tokens` holds the Stardust lexer's view of the
//! token stream. The test then asserts it matches the Rust lexer's
//! `Vec<LexedToken>` modulo well-documented acceptable differences
//! (catalogued in SELFHOST_V0_4_NOTES.md).

use sdust_driver::{lower, lower_to_sir, parse_source, type_and_borrow_check};
use sdust_sir::interp::{run_fn_by_name, Host, RunResult, Value};
use sdust_sir::sir::EffectOp;
use sdust_syntax::{lex as rust_lex, SyntaxKind};
use sdust_types::{EffectId, IntKind};
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

// ---- Selfhost host ------------------------------------------------------

#[derive(Debug, Default)]
struct SelfhostHost {
    src: Vec<u8>,
    stdout: Vec<u8>,
    tokens: Vec<TokenRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TokenRecord {
    kind: String,
    start: usize,
    end: usize,
}

impl Host for SelfhostHost {
    fn print(&mut self, s: &str) {
        self.stdout.extend_from_slice(s.as_bytes());
    }

    fn effect_call(&mut self, _effect: EffectId, op: &EffectOp, args: &[Value]) -> Value {
        // The lexer reaches the host via `std.io.<method>(...)` calls
        // which the SIR lowerer turns into `EffectOp::GenericCall`.
        // We service the v0.4 selfhost bridge here; anything else
        // returns Unit (matching BufferHost's permissive default).
        let EffectOp::GenericCall { method, .. } = op;
        self.dispatch_method(method, args)
    }

    fn extern_call(&mut self, _name: &str, _args: &[Value]) -> Value {
        // The selfhost bridge goes through effect_call, not extern_call.
        // Keep this stubbed for forward-compat with the v0.5 plan that
        // splits the bridge into real extern fns.
        Value::Unit
    }
}

impl SelfhostHost {
    fn dispatch_method(&mut self, method: &str, args: &[Value]) -> Value {
        match method {
            "lex_init" => {
                let s = args.first().map(value_as_str).unwrap_or_default();
                self.src = s.into_bytes();
                Value::Unit
            }
            "lex_len" => Value::Int(self.src.len() as i128, IntKind::USize),
            "lex_byte_at" => {
                let i = args.first().and_then(|v| v.as_int()).unwrap_or(0) as usize;
                let b = if i < self.src.len() {
                    self.src[i] as i128
                } else {
                    256
                };
                Value::Int(b, IntKind::U32)
            }
            "lex_slice" => {
                let s = args.first().and_then(|v| v.as_int()).unwrap_or(0) as usize;
                let e = args.get(1).and_then(|v| v.as_int()).unwrap_or(0) as usize;
                let lo = s.min(self.src.len());
                let hi = e.min(self.src.len()).max(lo);
                let bytes = &self.src[lo..hi];
                let text = std::str::from_utf8(bytes).unwrap_or("").to_string();
                Value::Str(text)
            }
            "lex_emit" => {
                let kind = args.first().map(value_as_str).unwrap_or_default();
                let start = args.get(1).and_then(|v| v.as_int()).unwrap_or(0) as usize;
                let end = args.get(2).and_then(|v| v.as_int()).unwrap_or(0) as usize;
                self.tokens.push(TokenRecord { kind, start, end });
                Value::Unit
            }
            _ => Value::Unit,
        }
    }
}

fn value_as_str(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Char(c) => c.to_string(),
        _ => v.as_str(),
    }
}

// ---- Compile + run the self-hosted lexer --------------------------------

fn run_selfhost_lexer(input: &str) -> Result<Vec<TokenRecord>, String> {
    let lexer_path = workspace_root().join("selfhost/lexer/lexer.sd");
    let lexer_src = std::fs::read_to_string(&lexer_path)
        .map_err(|e| format!("read {}: {}", lexer_path.display(), e))?;
    let parsed = parse_source(lexer_src, "selfhost/lexer/lexer.sd".into());
    let (pkg, lower_diags) = lower(&parsed);
    if lower_diags
        .iter()
        .any(|d| matches!(d.severity, sdust_diagnostics::Severity::Error))
    {
        return Err(format!("lower errors: {:?}", lower_diags));
    }
    let tbc = type_and_borrow_check(&pkg);
    let any_err = tbc
        .iter()
        .any(|d| matches!(d.severity, sdust_diagnostics::Severity::Error));
    if any_err {
        return Err(format!(
            "type/borrow errors: {:?}",
            tbc.iter()
                .filter(|d| matches!(d.severity, sdust_diagnostics::Severity::Error))
                .collect::<Vec<_>>()
        ));
    }
    let (prog, sir_diags) = lower_to_sir(&pkg);
    if sir_diags
        .iter()
        .any(|d| matches!(d.severity, sdust_diagnostics::Severity::Error))
    {
        return Err(format!("sir errors: {:?}", sir_diags));
    }

    let mut host = SelfhostHost::default();
    let res = run_fn_by_name(
        &prog,
        "lex",
        vec![Value::Str(input.to_string())],
        &mut host,
    );
    match res {
        Ok(_) => Ok(host.tokens),
        Err(RunResult::Trap { code, message, .. }) => {
            Err(format!("Stardust lexer trapped: {} {}", code, message))
        }
        Err(other) => Err(format!("Stardust lexer failed: {:?}", other)),
    }
}

// ---- Diff against the Rust reference impl -------------------------------

fn rust_lex_records(input: &str) -> Vec<TokenRecord> {
    rust_lex(input)
        .into_iter()
        .map(|t| TokenRecord {
            kind: format!("{:?}", t.kind),
            start: t.start,
            end: t.end,
        })
        .collect()
}

fn diff_summary(actual: &[TokenRecord], expected: &[TokenRecord]) -> String {
    let mut lines = Vec::new();
    let n = actual.len().max(expected.len());
    for i in 0..n {
        let a = actual.get(i);
        let e = expected.get(i);
        if a != e {
            lines.push(format!("  [{}] stardust={:?}  rust={:?}", i, a, e));
            if lines.len() > 20 {
                lines.push("  …".into());
                break;
            }
        }
    }
    lines.join("\n")
}

// ---- Tests --------------------------------------------------------------

#[test]
fn selfhost_lexer_compiles() {
    // Sanity: just compile the lexer source through the v0.3 pipeline.
    // If this fails, the source has type errors and the diff tests below
    // would give an opaque "lower errors" message.
    let lexer_path = workspace_root().join("selfhost/lexer/lexer.sd");
    let src = std::fs::read_to_string(&lexer_path).expect("read lexer.sd");
    let parsed = parse_source(src, "selfhost/lexer/lexer.sd".into());
    let (pkg, diags) = lower(&parsed);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, sdust_diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "lower errors: {:?}", errors);
    let tbc = type_and_borrow_check(&pkg);
    let tbc_errors: Vec<_> = tbc
        .iter()
        .filter(|d| matches!(d.severity, sdust_diagnostics::Severity::Error))
        .collect();
    assert!(
        tbc_errors.is_empty(),
        "type/borrow errors in selfhost lexer: {:?}",
        tbc_errors
    );
}

#[test]
fn selfhost_lexer_first_token_matches() {
    // v0.4 PARTIAL bootstrap: the Stardust lexer reaches the host via
    // the std.io effect bridge, scans the first token correctly, and
    // emits it through `lex_emit`. The full token stream is gated on
    // the v0.5 interpreter's iterative-loop fix (the v0.3 SIR lowerer
    // emits while/loop/for as single-iteration — see
    // SELFHOST_V0_4_NOTES.md "loops are single-iteration in v0.3").
    //
    // We assert exactly two emits land:
    //   1. the first token (FN_KW for `fn`)
    //   2. the trailing EOF (emitted after the main loop, regardless
    //      of how many iterations the loop actually performed)
    let input = "fn main() { log(\"hi\") }";
    let actual = run_selfhost_lexer(input).expect("Stardust lexer execution should not trap");

    // First emitted token is always the lead one — matches Rust.
    let expected_first = rust_lex_records(input)
        .into_iter()
        .next()
        .expect("Rust lexer produces at least one token");
    assert!(
        !actual.is_empty(),
        "Stardust lexer emitted no tokens — host bridge broken?"
    );
    assert_eq!(
        actual[0], expected_first,
        "first-token mismatch: stardust={:?} rust={:?}",
        actual[0], expected_first
    );
    // Trailing token is EOF.
    let last = actual.last().unwrap();
    assert_eq!(last.kind, "EOF", "last emitted token should be EOF");
    assert_eq!(
        last.end,
        input.len(),
        "EOF span end should match source length"
    );
}

#[test]
#[ignore = "v0.5 — gated on iterative-loop interpreter fix (SELFHOST_V0_4_NOTES.md)"]
fn selfhost_lexer_full_diff_against_rust() {
    // Full v0.5 acceptance: every emitted token (kind + start + end)
    // matches the Rust reference impl byte-for-byte. When the v0.5
    // interpreter lifts the single-iteration loop limitation, remove
    // the `#[ignore]` to enable this assertion.
    let input = "fn main() { log(\"hi\") }";
    let actual = run_selfhost_lexer(input).expect("Stardust lexer should run");
    let expected = rust_lex_records(input);
    assert_eq!(
        actual,
        expected,
        "self-hosted lexer disagrees with Rust lexer:\n{}",
        diff_summary(&actual, &expected)
    );
}

#[test]
fn rust_lexer_kind_names_stable() {
    // The bootstrap diff compares kind strings. This locks in the Rust
    // side's name formatting so a SyntaxKind rename doesn't silently
    // break the contract.
    let tokens = rust_lex("fn x() {}");
    let names: Vec<String> = tokens.iter().map(|t| format!("{:?}", t.kind)).collect();
    let expected = [
        "FN_KW",
        "WHITESPACE",
        "IDENT",
        "L_PAREN",
        "R_PAREN",
        "WHITESPACE",
        "L_BRACE",
        "R_BRACE",
        "EOF",
    ];
    assert_eq!(names.len(), expected.len(), "names = {:?}", names);
    for (i, want) in expected.iter().enumerate() {
        assert_eq!(&names[i], want);
    }
    // Sanity: the trailing token is EOF with zero-width span at end-of-source.
    let last = tokens.last().unwrap();
    assert_eq!(last.kind, SyntaxKind::EOF);
    assert_eq!(last.start, last.end);
}
