//! v0.34 T4 — MT4099 emit-site span fidelity.
//!
//! Pre-v0.34, the taint pass emitted MT4099 with a zero-span label
//! (`start=0,end=0`), so pretty + JSON output pointed at the start of
//! the file. The user-visible effect: hovering on the file's first
//! character to see a sink-violation that lives 30 lines below.
//!
//! This test file pins the new behaviour: the MT4099 envelope's
//! primary label points at the SINK ARGUMENT's source range — the
//! tainted value reference, not the file start.
//!
//! The fix lives in `mty_types::taint::TaintCx::report_sink_if_tainted`;
//! it threads each call's argument-expression spans (recorded by the
//! expression lowerer on every `ExprId`) into the diagnostic Label.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

/// Run the full parse → lower → type-check pipeline and return the
/// first MT4099 diagnostic (None if none was emitted).
fn first_mt4099(src: &str) -> Option<mty_diagnostics::Diagnostic> {
    let parsed = parse_source(src.into(), "taint_span_fidelity.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    let any_lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !any_lower_err {
        diags.extend(check_package(&pkg));
    }
    diags.into_iter().find(|d| d.code.as_str() == "MT4099")
}

// ----------------------------------------------------------------------
// 1. MT4099 span is non-zero (sentinel of "real span recorded").
// ----------------------------------------------------------------------

#[test]
fn mt4099_span_is_nonzero() {
    let src = r#"
        use std.swarm
        use std.fs
        fn main() {
          let m = Member.anthropic("claude")
          let reply = m.ask("hello")
          std.fs.write("/tmp/log.txt", reply)
        }
    "#;
    let d = first_mt4099(src).expect("MT4099 must fire");
    assert!(
        d.primary.start != 0 || d.primary.end != 0,
        "expected non-zero span, got start={} end={}",
        d.primary.start,
        d.primary.end,
    );
    assert!(
        d.primary.end > d.primary.start,
        "span end must exceed start: start={} end={}",
        d.primary.start,
        d.primary.end,
    );
}

// ----------------------------------------------------------------------
// 2. The span actually points at the tainted-arg substring.
// ----------------------------------------------------------------------

#[test]
fn mt4099_span_covers_sink_arg_reference() {
    // The sink arg is the bare local `reply`; the span must slice the
    // source back to exactly that identifier.
    let src = r#"
        use std.swarm
        use std.fs
        fn main() {
          let m = Member.anthropic("claude")
          let reply = m.ask("hello")
          std.fs.write("/tmp/log.txt", reply)
        }
    "#;
    let d = first_mt4099(src).expect("MT4099 must fire");
    let slice = &src[d.primary.start..d.primary.end];
    assert_eq!(
        slice, "reply",
        "MT4099 span must slice the sink arg identifier, got {:?}",
        slice
    );
}

// ----------------------------------------------------------------------
// 3. env.var → fs.write: span points at the tainted local.
// ----------------------------------------------------------------------

#[test]
fn mt4099_span_for_env_var_sink_arg() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let user_input = std.env.var("USER")
          std.fs.write("/tmp/log.txt", user_input)
        }
    "#;
    let d = first_mt4099(src).expect("MT4099 must fire");
    let slice = &src[d.primary.start..d.primary.end];
    assert_eq!(
        slice, "user_input",
        "expected the sink-arg identifier, got {:?}",
        slice,
    );
}

// ----------------------------------------------------------------------
// 4. Span on a multi-arg sink picks the SENSITIVE arg, not arg 0.
// ----------------------------------------------------------------------

#[test]
fn mt4099_span_targets_sensitive_arg_only() {
    // `std.fs.write(path, contents)` — `contents` (arg index 1) is the
    // sensitive position. The span must point at `tainted`, not at
    // the clean `path` literal.
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let tainted = std.env.var("HOSTILE")
          std.fs.write("/tmp/p.txt", tainted)
        }
    "#;
    let d = first_mt4099(src).expect("MT4099 must fire");
    let slice = &src[d.primary.start..d.primary.end];
    assert_eq!(
        slice, "tainted",
        "span must point at the sensitive arg, got {:?}",
        slice,
    );
}

// ----------------------------------------------------------------------
// 5. SQL sink (free-fn shape) also gets a tight span.
// ----------------------------------------------------------------------

#[test]
fn mt4099_span_for_sql_execute() {
    let src = r#"
        use std.env
        use std.sql
        fn main() {
          let q = std.env.var("QUERY")
          std.sql.execute(q)
        }
    "#;
    let d = first_mt4099(src).expect("MT4099 must fire");
    let slice = &src[d.primary.start..d.primary.end];
    assert_eq!(
        slice, "q",
        "expected the sql.execute arg identifier, got {:?}",
        slice,
    );
    assert!(
        d.primary.start != 0 || d.primary.end != 0,
        "span must be non-zero",
    );
}

// ----------------------------------------------------------------------
// 6. Multiple MT4099s in one file each get their own arg-tight span.
// ----------------------------------------------------------------------

#[test]
fn multiple_mt4099_have_distinct_spans() {
    let src = r#"
        use std.env
        use std.fs
        use std.sql
        fn main() {
          let first = std.env.var("A")
          let second = std.env.var("B")
          std.fs.write("/tmp/a.txt", first)
          std.sql.execute(second)
        }
    "#;
    let parsed = parse_source(src.into(), "multi.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    diags.extend(check_package(&pkg));
    let mt4099s: Vec<_> = diags
        .iter()
        .filter(|d| d.code.as_str() == "MT4099")
        .collect();
    assert!(
        mt4099s.len() >= 2,
        "expected at least 2 MT4099 diagnostics, got {}",
        mt4099s.len(),
    );
    // Each diagnostic's span must be non-zero AND distinct from the
    // others — proves the spans aren't all collapsed to (0, 0).
    let spans: std::collections::HashSet<(usize, usize)> = mt4099s
        .iter()
        .map(|d| (d.primary.start, d.primary.end))
        .collect();
    assert_eq!(
        spans.len(),
        mt4099s.len(),
        "every MT4099 should have a distinct span, got {:?}",
        spans,
    );
    for d in &mt4099s {
        assert!(
            d.primary.start != 0 || d.primary.end != 0,
            "no MT4099 should have a zero span",
        );
    }
}
