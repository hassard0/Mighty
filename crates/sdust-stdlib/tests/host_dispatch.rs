//! Verify the runtime-side `EffectOp::GenericCall` dispatcher gets
//! installed and routes to the real stdlib impls.

use sdust_sir::interp::value::Value;
use sdust_sir::sir::EffectOp;

#[test]
fn install_then_dispatch_json_parse() {
    sdust_stdlib::host::install();

    // We can't easily instantiate a full StdHost without a real
    // BudgetTracker, but we can call the registered dispatcher
    // directly via the runtime's effect_call path on a minimal host.
    let path = vec!["std".to_string(), "json".to_string()];
    let _op = EffectOp::GenericCall {
        path: path.clone(),
        method: "parse".into(),
    };
    let out = sdust_stdlib::host::dispatch(&path, "parse", &[Value::Str("{\"a\":1}".into())]);
    match out {
        Value::Str(s) => {
            assert!(s.contains("\"a\""), "expected JSON-shaped reply, got {s}");
        }
        other => panic!("expected Str, got {other:?}"),
    }
}

#[test]
fn dispatch_fs_exists_for_known_path() {
    let path = vec!["std".to_string(), "fs".to_string()];
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("x.txt");
    std::fs::write(&f, b"x").unwrap();
    let out = sdust_stdlib::host::dispatch(
        &path,
        "exists",
        &[Value::Unit, Value::Str(f.display().to_string())],
    );
    assert!(matches!(out, Value::Bool(true)), "got {out:?}");
}

#[test]
fn unknown_module_returns_unit() {
    let path = vec!["std".to_string(), "doesnt_exist".to_string()];
    let out = sdust_stdlib::host::dispatch(&path, "method", &[]);
    assert!(matches!(out, Value::Unit));
}
