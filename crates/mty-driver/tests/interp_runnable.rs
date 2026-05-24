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
    let parsed = parse_source(src.to_string(), "test.sd".into());
    let (pkg, _) = lower(&parsed);
    let (prog, _) = lower_to_sir(&pkg);
    let mut host = BufferHost::default();
    let res = run(&prog, &mut host);
    (res, host.stdout_str())
}

#[test]
fn hello_world_prints() {
    let path = workspace_root().join("examples/01_hello.sd");
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
