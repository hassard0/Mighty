//! Integration test: every canonical example in `examples/` must
//! type-check clean (lex + parse + lower + type-check produce no errors).
//!
//! Warnings are allowed (slice 3 emits SD2015 non-exhaustive-match as a
//! warning, etc.).

use sdust_diagnostics::Severity;
use sdust_driver::{lower, parse_source, type_check};

fn workspace_root() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR points to crates/sdust-driver; walk up twice.
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

fn check_example(name: &str) -> Vec<sdust_diagnostics::Diagnostic> {
    let path = workspace_root().join("examples").join(name);
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("failed to read {}: {}", path.display(), e);
    });
    let parsed = parse_source(src, path.display().to_string());
    let (pkg, mut diags) = lower(&parsed);
    diags.extend(type_check(&pkg));
    diags
}

fn assert_clean(name: &str) {
    let diags = check_example(name);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "example {} should type-check clean; got {} errors: {:?}",
        name,
        errors.len(),
        errors
            .iter()
            .map(|d| format!("{}: {}", d.code.as_str(), d.primary.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn example_01_hello() {
    assert_clean("01_hello.sd");
}
#[test]
fn example_02_struct_enum() {
    assert_clean("02_struct_enum.sd");
}
#[test]
fn example_03_generic_fn() {
    assert_clean("03_generic_fn.sd");
}
#[test]
fn example_04_result_propagation() {
    assert_clean("04_result_propagation.sd");
}
#[test]
fn example_05_match_expr() {
    assert_clean("05_match_expr.sd");
}
#[test]
fn example_06_for_while_loop() {
    assert_clean("06_for_while_loop.sd");
}
#[test]
fn example_07_agent_echo() {
    assert_clean("07_agent_echo.sd");
}
#[test]
fn example_08_agent_state() {
    assert_clean("08_agent_state.sd");
}
#[test]
fn example_09_send_ask_deadline() {
    assert_clean("09_send_ask_deadline.sd");
}
#[test]
fn example_10_supervisor() {
    assert_clean("10_supervisor.sd");
}
#[test]
fn example_11_budget_block() {
    assert_clean("11_budget_block.sd");
}
#[test]
fn example_12_arena() {
    assert_clean("12_arena.sd");
}
#[test]
fn example_13_capabilities() {
    assert_clean("13_capabilities.sd");
}
#[test]
fn example_14_extern_c() {
    assert_clean("14_extern_c.sd");
}
#[test]
fn example_15_extern_js() {
    assert_clean("15_extern_js.sd");
}
#[test]
fn example_16_macro() {
    assert_clean("16_macro.sd");
}
#[test]
fn example_17_unsafe() {
    assert_clean("17_unsafe.sd");
}
#[test]
fn example_18_sandbox() {
    assert_clean("18_sandbox.sd");
}
#[test]
fn example_19_backend_service() {
    assert_clean("19_backend_service.sd");
}
#[test]
fn example_20_frontend_component() {
    assert_clean("20_frontend_component.sd");
}
