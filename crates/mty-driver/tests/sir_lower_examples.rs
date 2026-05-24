//! Smoke test: every example lowers all the way to SIR without panic.

use mty_driver::{lower, lower_to_sir, parse_source};
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

fn lower_one(name: &str) -> mty_ir::Program {
    let path = workspace_root().join("examples").join(name);
    let src = std::fs::read_to_string(&path).unwrap();
    let parsed = parse_source(src, path.display().to_string());
    let (pkg, _diags) = lower(&parsed);
    let (prog, _) = lower_to_sir(&pkg);
    prog
}

#[test]
fn example_01_hello_lowers() {
    let p = lower_one("01_hello.mty");
    assert!(p.fn_by_name("main").is_some(), "main should be present");
}

#[test]
fn all_examples_lower_without_panic() {
    let examples = [
        "01_hello.mty",
        "02_struct_enum.mty",
        "03_generic_fn.mty",
        "04_result_propagation.mty",
        "05_match_expr.mty",
        "06_for_while_loop.mty",
        "07_agent_echo.mty",
        "08_agent_state.mty",
        "09_send_ask_deadline.mty",
        "10_supervisor.mty",
        "11_budget_block.mty",
        "12_arena.mty",
        "13_capabilities.mty",
        "14_extern_c.mty",
        "15_extern_js.mty",
        "16_macro.mty",
        "17_unsafe.mty",
        "18_sandbox.mty",
        "19_backend_service.mty",
        "20_frontend_component.mty",
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
    let p = lower_one("07_agent_echo.mty");
    assert!(
        p.agent_by_name("Echoer").is_some(),
        "Echoer agent should be registered"
    );
}

#[test]
fn struct_enum_lowers_adts() {
    let p = lower_one("02_struct_enum.mty");
    let names: Vec<&str> = p.adts.iter().map(|a| a.name.as_str()).collect();
    assert!(names.contains(&"User"), "User struct missing");
    assert!(names.contains(&"Shape"), "Shape enum missing");
}
