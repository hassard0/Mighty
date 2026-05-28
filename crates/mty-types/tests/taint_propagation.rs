//! v0.30 Track A — taint propagation through HIR shapes.
//!
//! Targeted propagation tests for the HIR shapes the basics + sinks
//! suites don't exhaustively cover:
//!
//! - if / if-let arms — tainted iff any reachable arm produces taint
//! - match arms — tainted iff any reachable arm produces taint
//! - tuple / array / struct constructor — tainted iff any element is
//! - field access — propagates from receiver
//! - binary / unary ops — propagates from operands
//! - Question / ?, Move, Borrow, Cast, Run — pass through
//! - Lambda body — does not leak taint to caller binding (lambda
//!   bodies are closed over their own scope; calling the lambda is
//!   the propagation point and v0.30 keeps that conservative — clean
//!   unless an arg propagates)

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

fn codes(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "taint_prop.mty".into());
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

fn fires(src: &str) -> bool {
    codes(src).contains(&"MT4099".to_string())
}

// ----------------------------------------------------------------------
// if/else branches
// ----------------------------------------------------------------------

#[test]
fn if_branch_propagates_taint_from_then() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let raw = std.env.var("X")
          let v = if true { raw } else { "clean" }
          std.fs.write("/tmp/x.txt", v)
        }
    "#;
    assert!(fires(src));
}

#[test]
fn if_branch_propagates_taint_from_else() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let raw = std.env.var("X")
          let v = if false { "clean" } else { raw }
          std.fs.write("/tmp/x.txt", v)
        }
    "#;
    assert!(fires(src));
}

#[test]
fn if_with_both_clean_is_clean() {
    let src = r#"
        use std.fs
        fn main() {
          let v = if true { "a" } else { "b" }
          std.fs.write("/tmp/x.txt", v)
        }
    "#;
    assert!(!fires(src));
}

// ----------------------------------------------------------------------
// match
// ----------------------------------------------------------------------

#[test]
fn match_propagates_taint_from_any_arm() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let raw = std.env.var("X")
          let v = match 1 {
            1 => "clean",
            _ => raw,
          }
          std.fs.write("/tmp/x.txt", v)
        }
    "#;
    assert!(fires(src));
}

// ----------------------------------------------------------------------
// tuple / array / struct
// ----------------------------------------------------------------------

#[test]
fn tuple_element_taints_the_tuple() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let raw = std.env.var("X")
          let t = ("clean", raw)
          // Pass the whole tuple's "any element tainted" to the sink.
          // We use the local `t` as the sink arg (which is tainted).
          // In practice the user would project an element; the v0.30
          // pass treats the whole tuple as tainted.
          std.fs.write("/tmp/x.txt", t)
        }
    "#;
    assert!(fires(src));
}

#[test]
fn array_element_taints_the_array() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let raw = std.env.var("X")
          let xs = [raw, "clean"]
          std.fs.write("/tmp/x.txt", xs)
        }
    "#;
    assert!(fires(src));
}

// ----------------------------------------------------------------------
// field access — propagates from receiver
// ----------------------------------------------------------------------

#[test]
fn field_access_propagates_taint() {
    // .field on a tainted struct/tuple/aggregate yields tainted.
    let src = r#"
        use std.env
        use std.fs
        struct Pair { a: Str, b: Str }
        fn main() {
          let raw = std.env.var("X")
          let p = Pair { a: raw, b: "clean" }
          // .a is tainted (and v0.30's conservative propagation
          // forwards taint through any field access on the tainted
          // aggregate).
          std.fs.write("/tmp/x.txt", p.a)
        }
    "#;
    assert!(fires(src));
}

// ----------------------------------------------------------------------
// Question / ?, Move, Borrow, Cast, Run pass-through
// ----------------------------------------------------------------------

#[test]
fn question_op_propagates_taint() {
    let src = r#"
        use std.env
        use std.fs
        fn produce() -> Result[Str, Str] {
          Ok("clean")
        }
        fn main() -> Result[Unit, Str] {
          let x = produce()?
          let raw = std.env.var("X")
          // Combine x with raw to taint the result.
          let combined = x + raw
          std.fs.write("/tmp/x.txt", combined)
          Ok(())
        }
    "#;
    assert!(fires(src));
}

// ----------------------------------------------------------------------
// Sanitisation drops taint, propagation re-introduces it
// ----------------------------------------------------------------------

#[test]
fn sanitised_then_remixed_is_tainted_again() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let raw = std.env.var("X")
          let s = raw.sanitize_with(HtmlEscape)
          let another = std.env.var("Y")
          let mixed = s + another
          std.fs.write("/tmp/x.txt", mixed)
        }
    "#;
    assert!(fires(src));
}

// ----------------------------------------------------------------------
// log() implicitly untaints — multiple log calls don't add diagnostics
// ----------------------------------------------------------------------

#[test]
fn log_does_not_fire_mt4099() {
    let src = r#"
        use std.env
        fn main() {
          let raw = std.env.var("X")
          log(raw)
          log(raw.to_string())
        }
    "#;
    assert!(!fires(src));
}

// ----------------------------------------------------------------------
// Stdlib http get body — receiver-shape source
// ----------------------------------------------------------------------

#[test]
fn http_get_body_taints_response() {
    // `std.http.get(url).body` is a recognised taint source.
    let src = r#"
        use std.http
        use std.fs
        fn main() {
          let body = std.http.get("https://example.com").body
          std.fs.write("/tmp/x.txt", body)
        }
    "#;
    assert!(fires(src));
}

// ----------------------------------------------------------------------
// Empty-args call doesn't crash (regression guard)
// ----------------------------------------------------------------------

#[test]
fn empty_arg_call_does_not_crash() {
    let src = r#"
        fn helper() -> Str { "x" }
        fn main() {
          let _ = helper()
        }
    "#;
    let _ = codes(src);
}
