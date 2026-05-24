//! Drive `examples/*.sd` through lex+parse+lower+typeck+borrowck and assert
//! no Error diagnostics surface.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source, type_and_borrow_check};
use std::path::PathBuf;

fn examples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("examples");
    p
}

fn check_clean(name: &str) {
    let path = examples_dir().join(name);
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("can't read {}: {}", path.display(), e);
    });
    let parsed = parse_source(src, path.display().to_string());
    let (pkg, mut diags) = lower(&parsed);
    let lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !lower_err {
        diags.extend(type_and_borrow_check(&pkg));
    }
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "{}: expected clean but got {} error(s): {:?}",
        name,
        errors.len(),
        errors
            .iter()
            .map(|d| format!("{}={}", d.code.as_str(), d.primary.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn ex_01() {
    check_clean("01_hello.sd");
}
#[test]
fn ex_02() {
    check_clean("02_struct_enum.sd");
}
#[test]
fn ex_03() {
    check_clean("03_generic_fn.sd");
}
#[test]
fn ex_04() {
    check_clean("04_result_propagation.sd");
}
#[test]
fn ex_05() {
    check_clean("05_match_expr.sd");
}
#[test]
fn ex_06() {
    check_clean("06_for_while_loop.sd");
}
#[test]
fn ex_07() {
    check_clean("07_agent_echo.sd");
}
#[test]
fn ex_08() {
    check_clean("08_agent_state.sd");
}
#[test]
fn ex_09() {
    check_clean("09_send_ask_deadline.sd");
}
#[test]
fn ex_10() {
    check_clean("10_supervisor.sd");
}
#[test]
fn ex_11() {
    check_clean("11_budget_block.sd");
}
#[test]
fn ex_12() {
    check_clean("12_arena.sd");
}
#[test]
fn ex_13() {
    check_clean("13_capabilities.sd");
}
#[test]
fn ex_14() {
    check_clean("14_extern_c.sd");
}
#[test]
fn ex_15() {
    check_clean("15_extern_js.sd");
}
#[test]
fn ex_16() {
    check_clean("16_macro.sd");
}
#[test]
fn ex_17() {
    check_clean("17_unsafe.sd");
}
#[test]
fn ex_18() {
    check_clean("18_sandbox.sd");
}
#[test]
fn ex_19() {
    check_clean("19_backend_service.sd");
}
#[test]
fn ex_20() {
    check_clean("20_frontend_component.sd");
}
