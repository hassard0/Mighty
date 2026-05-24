//! Smoke test: every example lowers all the way to SIR without panic.

use sdust_driver::{lower, lower_to_sir, parse_source};
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

fn lower_one(name: &str) -> sdust_sir::Program {
    let path = workspace_root().join("examples").join(name);
    let src = std::fs::read_to_string(&path).unwrap();
    let parsed = parse_source(src, path.display().to_string());
    let (pkg, _diags) = lower(&parsed);
    let (prog, _) = lower_to_sir(&pkg);
    prog
}

#[test]
fn example_01_hello_lowers() {
    let p = lower_one("01_hello.sd");
    assert!(p.fn_by_name("main").is_some(), "main should be present");
}

#[test]
fn all_examples_lower_without_panic() {
    let examples = [
        "01_hello.sd",
        "02_struct_enum.sd",
        "03_generic_fn.sd",
        "04_result_propagation.sd",
        "05_match_expr.sd",
        "06_for_while_loop.sd",
        "07_agent_echo.sd",
        "08_agent_state.sd",
        "09_send_ask_deadline.sd",
        "10_supervisor.sd",
        "11_budget_block.sd",
        "12_arena.sd",
        "13_capabilities.sd",
        "14_extern_c.sd",
        "15_extern_js.sd",
        "16_macro.sd",
        "17_unsafe.sd",
        "18_sandbox.sd",
        "19_backend_service.sd",
        "20_frontend_component.sd",
    ];
    for name in examples {
        let p = lower_one(name);
        // Every example must record at least an empty Program.
        // Examples without fns can have zero, but no panic is the
        // primary win — reaching this assert is the test.
        assert!(p.fns.len() <= 100, "example {} produced too many fns", name);
    }
}

#[test]
fn agent_examples_register_agents() {
    let p = lower_one("07_agent_echo.sd");
    assert!(
        p.agent_by_name("Echoer").is_some(),
        "Echoer agent should be registered"
    );
}

#[test]
fn struct_enum_lowers_adts() {
    let p = lower_one("02_struct_enum.sd");
    let names: Vec<&str> = p.adts.iter().map(|a| a.name.as_str()).collect();
    assert!(names.iter().any(|n| *n == "User"), "User struct missing");
    assert!(names.iter().any(|n| *n == "Shape"), "Shape enum missing");
}
