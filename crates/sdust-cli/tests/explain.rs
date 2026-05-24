use std::process::Command;

fn sdust(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_sdust"))
        .args(args)
        .output()
        .expect("run sdust");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn explain_known_code_succeeds() {
    let (code, stdout, _stderr) = sdust(&["explain", "SD0001"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Unexpected token"), "stdout: {}", stdout);
}

#[test]
fn explain_lowercase_prefix_works() {
    let (code, stdout, _) = sdust(&["explain", "sd0010"]);
    assert_eq!(code, 0);
    assert!(
        stdout.to_lowercase().contains("expected an item"),
        "stdout: {}",
        stdout
    );
}

#[test]
fn explain_bare_number_works() {
    let (code, stdout, _) = sdust(&["explain", "1001"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Unresolved name"), "stdout: {}", stdout);
}

#[test]
fn explain_unknown_code_fails() {
    let (code, _stdout, stderr) = sdust(&["explain", "SD9999"]);
    assert_eq!(code, 1);
    assert!(
        stderr.to_lowercase().contains("unknown"),
        "stderr: {}",
        stderr
    );
}

#[test]
fn explain_bad_format_fails() {
    let (code, _stdout, stderr) = sdust(&["explain", "wat"]);
    assert_eq!(code, 2);
    assert!(
        stderr.to_lowercase().contains("expected"),
        "stderr: {}",
        stderr
    );
}
