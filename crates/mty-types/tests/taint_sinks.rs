//! v0.30 Track A — sink-rejection tests.
//!
//! Each of the four documented sinks is exercised with a positive
//! (untainted accepted) and negative (tainted rejected with MT4099)
//! pair:
//!
//! - `std.fs.write(path, contents)`
//! - `process.Command.arg(arg)`
//! - `std.sql.execute(query)`
//! - `net.Request.body(body)`
//!
//! Plus: cross-sink (one taint flow → multiple sinks each report
//! separately), nested-sink (inside agent handler), and ctor-sourced
//! sinks (`Member.ask` → `Command.arg`).

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

fn diag_codes(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "taint_sinks.mty".into());
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

fn count_code(src: &str, code: &str) -> usize {
    diag_codes(src).iter().filter(|c| c == &code).count()
}

// ----------------------------------------------------------------------
// fs.write — positive + negative
// ----------------------------------------------------------------------

#[test]
fn fs_write_accepts_untainted_contents() {
    let src = r#"
        use std.fs
        fn main() {
          std.fs.write("/tmp/safe.txt", "hello")
        }
    "#;
    assert_eq!(count_code(src, "MT4099"), 0);
}

#[test]
fn fs_write_rejects_tainted_contents() {
    let src = r#"
        use std.fs
        use std.env
        fn main() {
          let raw = std.env.var("X")
          std.fs.write("/tmp/x.txt", raw)
        }
    "#;
    assert_eq!(count_code(src, "MT4099"), 1);
}

// ----------------------------------------------------------------------
// process.Command.arg — positive + negative
// ----------------------------------------------------------------------

#[test]
fn command_arg_accepts_untainted() {
    let src = r#"
        fn main() {
          let cmd = Command.new("ls")
          cmd.arg("-la")
        }
    "#;
    assert_eq!(count_code(src, "MT4099"), 0);
}

#[test]
fn command_arg_rejects_tainted() {
    let src = r#"
        use std.env
        fn main() {
          let raw = std.env.var("USER_QUERY")
          let cmd = Command.new("grep")
          cmd.arg(raw)
        }
    "#;
    assert_eq!(count_code(src, "MT4099"), 1);
}

// ----------------------------------------------------------------------
// sql.execute — positive + negative
// ----------------------------------------------------------------------

#[test]
fn sql_execute_accepts_untainted() {
    let src = r#"
        fn main() {
          std.sql.execute("SELECT 1")
        }
    "#;
    assert_eq!(count_code(src, "MT4099"), 0);
}

#[test]
fn sql_execute_rejects_tainted() {
    let src = r#"
        use std.env
        fn main() {
          let q = std.env.var("QUERY")
          std.sql.execute(q)
        }
    "#;
    assert_eq!(count_code(src, "MT4099"), 1);
}

// ----------------------------------------------------------------------
// net.Request.body — positive + negative
// ----------------------------------------------------------------------

#[test]
fn net_request_body_accepts_untainted() {
    let src = r#"
        fn main() {
          let req = Request.new("https://example.com")
          req.body("static-payload")
        }
    "#;
    assert_eq!(count_code(src, "MT4099"), 0);
}

#[test]
fn net_request_body_rejects_tainted() {
    let src = r#"
        use std.env
        fn main() {
          let raw = std.env.var("PAYLOAD")
          let req = Request.new("https://example.com")
          req.body(raw)
        }
    "#;
    assert_eq!(count_code(src, "MT4099"), 1);
}

// ----------------------------------------------------------------------
// Cross-sink: one tainted local → multiple sinks → multiple errors
// ----------------------------------------------------------------------

#[test]
fn one_tainted_value_flagged_at_each_sink() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let raw = std.env.var("EVIL")
          std.fs.write("/tmp/a.txt", raw)
          std.sql.execute(raw)
        }
    "#;
    // Each sink fires once; the first sink reports + clears the local
    // arg-taint flag, but the second sink re-checks via the local
    // (which is still tainted at the binding level), so both fire.
    assert_eq!(count_code(src, "MT4099"), 2);
}

// ----------------------------------------------------------------------
// Ctor-source chained: Member.anthropic → .ask() → fs.write
// ----------------------------------------------------------------------

#[test]
fn member_ask_chained_into_sink() {
    let src = r#"
        use std.swarm
        use std.fs
        fn main() {
          let m = Member.anthropic("claude-3")
          let reply = m.ask("write a poem")
          std.fs.write("/tmp/poem.txt", reply)
        }
    "#;
    assert_eq!(count_code(src, "MT4099"), 1);
}

// ----------------------------------------------------------------------
// Inside an agent handler — taint flows through Ask too
// ----------------------------------------------------------------------

#[test]
fn taint_flows_inside_agent_handler() {
    let src = r#"
        use std.fs
        use std.env
        protocol P { Do(x: Str) -> Str }
        agent A: P {
          on Do(x) -> {
            let raw = std.env.var("Y")
            std.fs.write("/tmp/y.txt", raw)
            "ok"
          }
        }
    "#;
    assert!(diag_codes(src).iter().any(|c| c == "MT4099"));
}

// ----------------------------------------------------------------------
// Tainted-through-format — propagation across method-call
// ----------------------------------------------------------------------

#[test]
fn format_string_propagates_taint() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let raw = std.env.var("X")
          let s = raw.to_string()
          std.fs.write("/tmp/x.txt", s)
        }
    "#;
    assert_eq!(count_code(src, "MT4099"), 1);
}

// ----------------------------------------------------------------------
// Tainted through a tuple / aggregate access — propagation
// ----------------------------------------------------------------------

#[test]
fn binary_op_propagates_taint() {
    let src = r#"
        use std.env
        use std.fs
        fn main() {
          let a = std.env.var("X")
          let b = "static"
          // `b + a` flows taint through the binary op (mixed).
          // The result is tainted because one operand was tainted.
          let mixed = b + a
          std.fs.write("/tmp/x.txt", mixed)
        }
    "#;
    assert_eq!(count_code(src, "MT4099"), 1);
}
