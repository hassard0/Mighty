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
