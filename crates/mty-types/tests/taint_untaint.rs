//! v0.30 Track A — untainting surface tests.
//!
//! Three sanctioned untainting strategies:
//!
//! 1. `value.matches_regex(pattern)` → `Option[Str]` of an untainted
//!    match (provably constrained by the regex shape).
//! 2. `value.in_allowlist[Enum]()` → `Option[Enum]` if the value
//!    parses as one of the enum's variant names.
//! 3. `value.sanitize_with(HtmlEscape | ShellEscape | SqlEscape |
//!    PathBoundary(...))` → applies a provably-correct sanitiser.
//!
//! All three return UNTAINTED values; the rest of the program can use
//! them with sinks freely. No escape hatch exists — there is no
//! `Tainted::unwrap_unchecked()`.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

fn diag_codes(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "taint_untaint.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    let any_lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !any_lower_err {
        diags.extend(check_package(&pkg));
    }
    diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| d.code.as_str())
        .collect()
}

// ----------------------------------------------------------------------
// matches_regex
// ----------------------------------------------------------------------

#[test]
fn matches_regex_yields_clean_value() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let raw = std.env.var("USERNAME")
          let clean = raw.matches_regex("^[a-zA-Z]+$").unwrap_or("anon")
          std.fs.write("/tmp/u.txt", clean)
        }
    "#;
    assert!(!diag_codes(src).contains(&"MT4099".to_string()));
}

#[test]
fn matches_regex_then_propagation_stays_clean() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let raw = std.env.var("X")
          let a = raw.matches_regex("^[a-z]+$").unwrap_or("safe")
          let b = a
          let c = b.to_string()
          std.fs.write("/tmp/x.txt", c)
        }
    "#;
    assert!(!diag_codes(src).contains(&"MT4099".to_string()));
}

// ----------------------------------------------------------------------
// in_allowlist
// ----------------------------------------------------------------------

#[test]
fn in_allowlist_yields_clean_value() {
    let src = r#"
        use std.env
        use std.fs
        enum Mode { Safe, Risky }
        fn main() {
          let raw = std.env.var("MODE")
          let mode = raw.in_allowlist().unwrap_or("safe")
          std.fs.write("/tmp/m.txt", mode)
        }
    "#;
    assert!(!diag_codes(src).contains(&"MT4099".to_string()));
}

// ----------------------------------------------------------------------
// sanitize_with — HtmlEscape / ShellEscape / SqlEscape / PathBoundary
// ----------------------------------------------------------------------

#[test]
fn sanitize_html_escape() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let raw = std.env.var("X")
          let safe = raw.sanitize_with(HtmlEscape)
          std.fs.write("/tmp/x.html", safe)
        }
    "#;
    assert!(!diag_codes(src).contains(&"MT4099".to_string()));
}

#[test]
fn sanitize_shell_escape() {
    let src = r#"
        use std.env
        fn main() {
          let raw = std.env.var("X")
          let safe = raw.sanitize_with(ShellEscape)
          let cmd = Command.new("echo")
          cmd.arg(safe)
        }
    "#;
    assert!(!diag_codes(src).contains(&"MT4099".to_string()));
}

#[test]
fn sanitize_sql_escape() {
    let src = r#"
        use std.env
        fn main() {
          let raw = std.env.var("Q")
          let q = raw.sanitize_with(SqlEscape)
          std.sql.execute(q)
        }
    "#;
    assert!(!diag_codes(src).contains(&"MT4099".to_string()));
}

#[test]
fn sanitize_path_boundary() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let raw = std.env.var("FILENAME")
          let safe_path = raw.sanitize_with(PathBoundary)
          std.fs.write(safe_path, "ok")
        }
    "#;
    assert!(!diag_codes(src).contains(&"MT4099".to_string()));
}

// ----------------------------------------------------------------------
// Sanitiser identifiers are recognised types
// ----------------------------------------------------------------------

#[test]
fn html_escape_is_in_scope() {
    let src = r#"
        fn main() {
          let _x: HtmlEscape = HtmlEscape
          let _y: ShellEscape = ShellEscape
          let _z: SqlEscape = SqlEscape
          let _p: PathBoundary = PathBoundary
        }
    "#;
    let codes = diag_codes(src);
    assert!(
        codes.iter().all(|c| c != "MT2002"),
        "sanitisers should resolve as types, got {:?}",
        codes
    );
}

// ----------------------------------------------------------------------
// No-op call on clean value — untainting fns work on already-clean too
// ----------------------------------------------------------------------

#[test]
fn untaint_methods_no_op_on_clean_values() {
    let src = r#"
        use std.fs
        fn main() {
          let clean = "already safe"
          let s1 = clean.matches_regex("^.*$").unwrap_or("")
          let s2 = clean.sanitize_with(HtmlEscape)
          std.fs.write("/tmp/a.txt", s1)
          std.fs.write("/tmp/b.txt", s2)
        }
    "#;
    assert!(!diag_codes(src).contains(&"MT4099".to_string()));
}

// ----------------------------------------------------------------------
// Untainting then re-tainting via mixing
// ----------------------------------------------------------------------

#[test]
fn taint_returns_when_remixed_with_tainted() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let raw1 = std.env.var("X")
          let raw2 = std.env.var("Y")
          // Untaint raw1.
          let clean1 = raw1.matches_regex("^[a-z]+$").unwrap_or("safe")
          // Mix clean1 with raw2 — result is tainted again.
          let mixed = clean1 + raw2
          std.fs.write("/tmp/x.txt", mixed)
        }
    "#;
    // The mixed value re-introduces taint because raw2 is tainted.
    assert!(diag_codes(src).contains(&"MT4099".to_string()));
}

// ----------------------------------------------------------------------
// Sanitiser-type bindings + chained sanitisation
// ----------------------------------------------------------------------

#[test]
fn chained_sanitisers() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let raw = std.env.var("INPUT")
          let html = raw.sanitize_with(HtmlEscape)
          // After sanitisation the value is clean; passing through a
          // second sanitiser is harmless.
          let again = html.sanitize_with(HtmlEscape)
          std.fs.write("/tmp/x.html", again)
        }
    "#;
    assert!(!diag_codes(src).contains(&"MT4099".to_string()));
}
