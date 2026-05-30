//! Runtime tests: a handful of examples with a runnable `fn main` are
//! executed under the interpreter and checked for expected stdout.

use mty_driver::{lower, lower_to_sir, parse_source};
use mty_ir::interp::{run, BufferHost, RunResult};
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

fn run_src(src: &str) -> (RunResult, String) {
    let parsed = parse_source(src.to_string(), "test.mty".into());
    let (pkg, _) = lower(&parsed);
    let (prog, _) = lower_to_sir(&pkg);
    let mut host = BufferHost::default();
    let res = run(&prog, &mut host);
    (res, host.stdout_str())
}

#[test]
fn hello_world_prints() {
    let path = workspace_root().join("examples/01_hello.mty");
    let src = std::fs::read_to_string(path).unwrap();
    let (res, out) = run_src(&src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    assert_eq!(out.trim_end(), "hello, Mighty");
}

#[test]
fn arithmetic_runs() {
    let src = r#"
        fn main() {
          let x = 1 + 2 * 3
          log(x.to_str())
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    assert_eq!(out.trim_end(), "7");
}

#[test]
fn if_chain_runs() {
    let src = r#"
        fn main() {
          let n = 5
          if n > 3 { log("big") } else { log("small") }
        }
    "#;
    let (_res, out) = run_src(src);
    assert_eq!(out.trim_end(), "big");
}

#[test]
fn let_and_print() {
    let src = r#"
        fn main() {
          let msg = "hi"
          log(msg)
        }
    "#;
    let (_res, out) = run_src(src);
    assert_eq!(out.trim_end(), "hi");
}

#[test]
fn no_main_returns_no_main_code() {
    let src = r#"fn other() { log("oops") }"#;
    let (res, _out) = run_src(src);
    assert!(matches!(res, RunResult::NoMain));
}

#[test]
fn panic_traps() {
    let src = r#"fn main() { panic("nope") }"#;
    let (res, _) = run_src(src);
    assert!(matches!(res, RunResult::Trap { .. }));
}

// ---- v0.29 Track D: while let lowering + interp ----

#[test]
fn while_let_immediately_exits_on_none() {
    // The scrutinee starts as `None`, so the loop body must NEVER
    // run. This pins down the pattern-fail -> exit transition in
    // `lower_while_let`: the first iteration's match fails, we jump
    // to the exit block, and `log("after")` runs once.
    let src = r#"
        fn pull() -> Option[I32] { None }
        fn main() {
          while let Some(_x) = pull() {
            log("BODY-RAN")
          }
          log("after")
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    let trimmed = out.trim_end();
    assert!(
        !trimmed.contains("BODY-RAN"),
        "body must not run: {:?}",
        out
    );
    assert!(trimmed.ends_with("after"), "expected after-line: {:?}", out);
}

#[test]
fn while_let_runs_body_when_match_succeeds() {
    // The scrutinee yields `Some(7)` on the first call, then the
    // body sets `done = true` and `break`s. This drives the
    // success-then-break path through the new lowering, exercising
    // both the per-iter pattern binding and the loop-frame goto.
    let src = r#"
        fn pull() -> Option[I32] { Some(7) }
        fn main() {
          let mut done = false
          while let Some(x) = pull() {
            log(x.to_str())
            done = true
            break
          }
          if done { log("done") } else { log("notdone") }
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    let lines: Vec<&str> = out.trim_end().lines().collect();
    assert_eq!(lines, vec!["7", "done"], "unexpected output: {:?}", out);
}

// ----------------------------------------------------------------------
// v0.41 T6 (L16) — top-level `const NAME: T = expr;` regression tests.
//
// Before T6, every `const` reference fell through `resolve_path` to
// `Operand::Const(Const::Unit)`, which the interpreter then default-coerced
// to the declared type's zero value. The IDE's `KIND_KW != 1_u8` check
// always fired because `KIND_KW` evaluated to `0_u8` regardless of the
// declared initializer. These tests assert the post-fix behaviour: the
// reference inlines the initializer at the use site.

#[test]
fn const_i32_inlined_in_log() {
    // L16 root case: the const must evaluate to its initializer rather
    // than the type's zero value. We test by direct equality (the `==`
    // path is what the IDE workaround relied on); `.to_str()` integer
    // formatting goes through a separate stdlib dispatch path that's
    // exercised by `arithmetic_runs`/`let_and_print` above.
    let src = r#"
        const ANSWER: I32 = 42
        fn main() {
          if ANSWER == 42 { log("ok") } else { log("bad") }
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    assert_eq!(out.trim_end(), "ok");
}

#[test]
fn const_u8_compares_correctly() {
    // Matches the L16 reproducer from the IDE: a `const KIND_KW: U8 = 1_u8`
    // should compare equal to `1_u8`, not the U8 default 0.
    let src = r#"
        const KIND_KW: U8 = 1_u8
        fn main() {
          if KIND_KW == 1_u8 { log("eq") } else { log("ne") }
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    assert_eq!(out.trim_end(), "eq");
}

#[test]
fn const_str_inlined() {
    let src = r#"
        const GREETING: String = "hello"
        fn main() {
          log(GREETING)
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    assert_eq!(out.trim_end(), "hello");
}

#[test]
fn const_bool_inlined() {
    let src = r#"
        const READY: Bool = true
        fn main() {
          if READY { log("yes") } else { log("no") }
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    assert_eq!(out.trim_end(), "yes");
}

#[test]
fn const_used_inside_fn() {
    // A non-main fn references the const; main calls the fn.
    let src = r#"
        const FACTOR: I32 = 10
        fn scale(x: I32) -> I32 { x * FACTOR }
        fn main() {
          log(scale(4).to_str())
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    assert_eq!(out.trim_end(), "40");
}

#[test]
fn const_used_as_default_arg_replacement() {
    // Const values used at multiple call sites should all evaluate to
    // the declared initializer.
    let src = r#"
        const BASE: I32 = 100
        fn add(a: I32, b: I32) -> I32 { a + b }
        fn main() {
          let v = add(BASE, BASE)
          log(v.to_str())
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    assert_eq!(out.trim_end(), "200");
}

#[test]
fn while_let_pattern_binding_visible_in_body() {
    // The pattern binding (`x`) must be in scope inside the body,
    // and only in the body. After `break`, the binding goes out of
    // scope but the loop's result lands at the exit block (Unit),
    // so the post-loop code still typechecks + runs.
    let src = r#"
        fn produce() -> Option[I32] { Some(42) }
        fn main() {
          let mut sum: I32 = 0
          while let Some(x) = produce() {
            sum = sum + x
            break
          }
          log(sum.to_str())
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    assert_eq!(out.trim_end(), "42");
}
