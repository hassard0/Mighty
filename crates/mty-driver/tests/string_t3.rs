//! End-to-end tests for the v0.36 Track T3 String surface.
//!
//! Each test feeds a Mighty source program through parse -> HIR ->
//! SIR lowering -> interpreter, then checks the emitted stdout. The
//! goal is to lock down the *source-level* shape of `find`, `rfind`,
//! `position`, `insert_at`, `remove_range`, `replace_range`,
//! `is_char_boundary`, `next_char_boundary`, `prev_char_boundary`,
//! `chars`, `char_indices`, `as_bytes`, and `byte_len` so any future
//! refactor that breaks the dispatch table gets caught here.

use mty_driver::{lower, lower_to_sir, parse_source};
use mty_ir::interp::{run, BufferHost, RunResult};

fn run_src(src: &str) -> (RunResult, String) {
    let parsed = parse_source(src.to_string(), "string_t3.mty".into());
    let (pkg, _) = lower(&parsed);
    let (prog, _) = lower_to_sir(&pkg);
    let mut host = BufferHost::default();
    let res = run(&prog, &mut host);
    (res, host.stdout_str())
}

#[test]
fn find_rfind_position_return_byte_indices() {
    // "Hello, Mighty Mighty" has "Mighty" at byte 7 and 14.
    let src = r#"
        fn main() {
          let s = "Hello, Mighty Mighty"
          match s.find("Mighty") {
            Some(i) => log(i.to_str())
            None => log("missing")
          }
          match s.rfind("Mighty") {
            Some(i) => log(i.to_str())
            None => log("missing")
          }
          match s.position('M') {
            Some(i) => log(i.to_str())
            None => log("missing")
          }
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    let lines: Vec<&str> = out.trim_end().lines().collect();
    assert_eq!(lines, vec!["7", "14", "7"]);
}

#[test]
fn insert_at_and_remove_range_splice_text() {
    let src = r#"
        fn main() {
          let s = "Hello, Mighty"
          match s.insert_at(7, "the ") {
            Some(t) => log(t)
            None => log("MT5080")
          }
          let edited = "Hello, the Mighty"
          match edited.remove_range(7, 11) {
            Some(t) => log(t)
            None => log("MT5080")
          }
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    let lines: Vec<&str> = out.trim_end().lines().collect();
    assert_eq!(lines, vec!["Hello, the Mighty", "Hello, Mighty"]);
}

#[test]
fn replace_range_swaps_substring() {
    let src = r#"
        fn main() {
          let s = "Hello, the Mighty"
          match s.replace_range(7, 11, "a ") {
            Some(t) => log(t)
            None => log("MT5080")
          }
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    assert_eq!(out.trim_end(), "Hello, a Mighty");
}

#[test]
fn range_ops_return_none_on_bad_boundary() {
    // 'é' is 2 bytes; byte index 1 is mid-sequence. The interp
    // dispatch surfaces this as `None` (the source-level companion to
    // the Rust-side `MT5080` panic).
    let src = r#"
        fn main() {
          let s = "é"
          match s.insert_at(1, "X") {
            Some(t) => log(t)
            None => log("MT5080-insert")
          }
          match s.remove_range(0, 1) {
            Some(t) => log(t)
            None => log("MT5080-remove")
          }
          match s.replace_range(0, 1, "z") {
            Some(t) => log(t)
            None => log("MT5080-replace")
          }
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    let lines: Vec<&str> = out.trim_end().lines().collect();
    assert_eq!(
        lines,
        vec!["MT5080-insert", "MT5080-remove", "MT5080-replace"]
    );
}

#[test]
fn char_boundary_helpers_navigate_multibyte() {
    // "a©b" — bytes 0 a, 1..3 ©, 3 b, 4 end.
    let src = r#"
        fn main() {
          let s = "a©b"
          if s.is_char_boundary(1) { log("yes") } else { log("no") }
          if s.is_char_boundary(2) { log("yes") } else { log("no") }
          match s.next_char_boundary(2) {
            Some(i) => log(i.to_str())
            None => log("end")
          }
          match s.prev_char_boundary(2) {
            Some(i) => log(i.to_str())
            None => log("start")
          }
          log(s.byte_len().to_str())
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    let lines: Vec<&str> = out.trim_end().lines().collect();
    assert_eq!(lines, vec!["yes", "no", "3", "1", "4"]);
}

#[test]
fn chars_and_char_indices_iterate_code_points() {
    let src = r#"
        fn main() {
          let s = "a©b"
          let xs = s.chars()
          log(xs.len().to_str())
          let pairs = s.char_indices()
          log(pairs.len().to_str())
        }
    "#;
    let (res, out) = run_src(src);
    assert_eq!(res, RunResult::Ok { exit: 0 });
    let lines: Vec<&str> = out.trim_end().lines().collect();
    assert_eq!(lines, vec!["3", "3"]);
}
