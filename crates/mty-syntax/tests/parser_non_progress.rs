//! Regression tests for the v0.9 FUZZ_V0_9 "non-progress-guard family"
//! parser bugs. Each of these inputs used to drive `mty_syntax::parse`
//! into an infinite loop that grew green-tree nodes until OOM
//! (~12 GB allocation request). After applying the audit-sweep
//! non-progress guards, every loop body in the parser bumps at least
//! one token per iteration even on adversarial input, so the parse
//! returns in microseconds with errors instead of hanging.
//!
//! The repros are kept tiny so a fresh contributor can paste them into
//! a debugger and step through the relevant `while !p.at(R_BRACE) &&
//! !p.at(EOF)` body to see the guard fire.

use std::time::{Duration, Instant};

/// Helper: parse with a wall-clock budget. Any input that takes more
/// than 5 seconds is treated as the regression having returned.
/// The actual successful path returns in microseconds.
fn parse_within(src: &str, budget: Duration) {
    let start = Instant::now();
    let _ = mty_syntax::parse(src);
    let elapsed = start.elapsed();
    assert!(
        elapsed < budget,
        "parse of {:?} took {:?}, exceeded budget {:?} — non-progress guard regressed?",
        src,
        elapsed,
        budget
    );
}

// Bug 1: enum-variant infinite loop on malformed payload.
// Pre-fix: ~12 GB, ~5 s before abort. Post-fix: microseconds.
#[test]
fn enum_malformed_payload_terminates() {
    parse_within("enum E { R(F>4)", Duration::from_secs(5));
}

#[test]
fn enum_empty_unclosed_terminates() {
    parse_within("enum E {", Duration::from_secs(5));
}

#[test]
fn enum_garbage_variant_terminates() {
    parse_within("enum E { @@@", Duration::from_secs(5));
}

// Bug 3: protocol_decl / protocol_msg infinite loop.
#[test]
fn protocol_malformed_msg_terminates() {
    parse_within("protocol P { Msg(F>4)", Duration::from_secs(5));
}

#[test]
fn protocol_garbage_body_terminates() {
    parse_within("protocol P { @@@", Duration::from_secs(5));
}

// Bug 3 sibling: the smaller typeck_fuzz repro from FUZZ_V0_9_NOTES.md.
// 96 bytes that OOM'd typeck_fuzz before the parser fix.
#[test]
fn typeck_fuzz_protocol_repro_terminates() {
    let src = "protocol Count {\n  In Shape {\n  Circle(F64)\n  Rect(F64, F64)\n = 0\n    on Inc() -> { n += 1; n }\n}\n";
    parse_within(src, Duration::from_secs(5));
}

// Audit-sweep regression: struct_decl with malformed field type.
// Same anti-pattern shape as enum_decl Bug 1.
#[test]
fn struct_malformed_field_terminates() {
    parse_within("struct S { x: F>4", Duration::from_secs(5));
}

// Audit-sweep regression: trait_decl with garbage body.
#[test]
fn trait_garbage_body_terminates() {
    parse_within("trait T { @@@", Duration::from_secs(5));
}

// Audit-sweep regression: impl_block with garbage body.
#[test]
fn impl_garbage_body_terminates() {
    parse_within("impl Foo { @@@", Duration::from_secs(5));
}

// Audit-sweep regression: extern block with garbage body.
#[test]
fn extern_garbage_body_terminates() {
    parse_within("extern c { @@@", Duration::from_secs(5));
}

// Audit-sweep regression: match expression with garbage arms.
#[test]
fn match_garbage_arms_terminates() {
    parse_within("fn f() { match x { @@@ } }", Duration::from_secs(5));
}

// Audit-sweep regression: supervisor decl with garbage body.
#[test]
fn supervisor_garbage_body_terminates() {
    parse_within("supervisor S { @@@", Duration::from_secs(5));
}

// Audit-sweep regression: top-level sandbox with garbage body.
#[test]
fn sandbox_garbage_body_terminates() {
    parse_within("sandbox X with { @@@", Duration::from_secs(5));
}

// Sanity: the well-formed counterparts still parse cleanly.
#[test]
fn enum_well_formed_still_parses() {
    let r = mty_syntax::parse("enum E { Red, Green, Blue }");
    assert_eq!(r.errors.len(), 0, "well-formed enum should parse cleanly");
}

// Direct replay of the saved fuzz artifacts (FUZZ_V0_9_NOTES.md). If
// the file isn't present (cargo-package run, or someone gitignored
// artifacts/), the test is a no-op rather than a failure.
#[test]
fn parser_fuzz_oom_artifacts_terminate() {
    let artifacts_dir = std::path::Path::new("fuzz/artifacts/parser_fuzz");
    if !artifacts_dir.exists() {
        eprintln!("skipping: {} not present", artifacts_dir.display());
        return;
    }
    let entries = std::fs::read_dir(artifacts_dir).expect("read_dir");
    let mut count = 0;
    for entry in entries {
        let entry = entry.expect("entry");
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !name.starts_with("oom-") && !name.starts_with("crash-") {
            continue;
        }
        let data = std::fs::read(&path).expect("read artifact");
        if let Ok(s) = std::str::from_utf8(&data) {
            parse_within(s, Duration::from_secs(5));
            count += 1;
        }
    }
    eprintln!("checked {} fuzz artifacts (parser_fuzz)", count);
}

#[test]
fn protocol_well_formed_still_parses() {
    // Protocol messages are newline-separated, not comma-separated.
    let src = "protocol P {\n  Ping() -> Pong\n  Hello(name: Str)\n}";
    let r = mty_syntax::parse(src);
    assert_eq!(
        r.errors.len(),
        0,
        "well-formed protocol should parse cleanly, got errors: {:?}",
        r.errors
    );
}
