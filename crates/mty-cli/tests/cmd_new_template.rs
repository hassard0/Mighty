//! v0.23 Track C — `mty new --template <name>` integration tests.
//!
//! Exercises the template registry end-to-end: scaffold the
//! web-game template, assert the four spec-mandated files appear,
//! then run `mty check` and `mty build --target wasm32-web` against
//! the freshly scaffolded package to confirm it actually compiles.
//!
//! See `dev/history/notes/MTY_SERVE_V0_23_NOTES.md`.

use std::process::Command;

fn mty(cwd: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_mty"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run mty");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn fresh_tmpdir(label: &str) -> std::path::PathBuf {
    let mut d = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    d.push(format!("mty-cli-test-{label}-{nanos}"));
    std::fs::create_dir_all(&d).expect("create tmpdir");
    d
}

#[test]
fn new_blank_default_template_still_works() {
    // Regression guard: no --template flag ⇒ same v0.1-shape scaffold.
    let dir = fresh_tmpdir("blank");
    let (code, stdout, stderr) = mty(&dir, &["new", "hello"]);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    let root = dir.join("hello");
    assert!(root.join("mighty.toml").is_file());
    assert!(root.join("src/main.mty").is_file());
    let manifest = std::fs::read_to_string(root.join("mighty.toml")).unwrap();
    assert!(manifest.contains("name = \"hello\""), "manifest={manifest}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_web_game_creates_expected_files() {
    // Spec: scaffold must produce src/main.mty, web/index.html,
    // web/dom-shim.js, mighty.toml.
    let dir = fresh_tmpdir("webgame-files");
    let (code, stdout, stderr) = mty(&dir, &["new", "--template", "web-game", "test-game"]);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    let root = dir.join("test-game");
    for f in [
        "mighty.toml",
        "src/main.mty",
        "web/index.html",
        "web/dom-shim.js",
    ] {
        assert!(
            root.join(f).is_file(),
            "expected scaffold file {f} to exist under {}",
            root.display()
        );
    }
    // Placeholder substitution worked everywhere we expect it.
    // Note: the user-supplied path `test-game` is sanitised into the
    // valid Mighty identifier `test_game` before being stamped into
    // `{{NAME}}`. This was a v0.23-integration fix: the previous code
    // pasted the raw arg in verbatim, which broke for any path with
    // hyphens, dots, or directory separators (e.g. `mty new /tmp/foo`
    // would produce `package /tmp/foo` — a parse error).
    let manifest = std::fs::read_to_string(root.join("mighty.toml")).unwrap();
    assert!(
        manifest.contains("name = \"test_game\""),
        "manifest didn't get the package name substituted: {manifest}"
    );
    let main = std::fs::read_to_string(root.join("src/main.mty")).unwrap();
    assert!(
        main.contains("package test_game"),
        "main.mty didn't get the package name substituted (expected `package test_game`)"
    );
    assert!(
        !main.contains("{{NAME}}"),
        "main.mty still contains a {{{{NAME}}}} placeholder"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_web_game_sanitises_path_to_identifier() {
    // Regression for v0.23 integration: scaffolding into a *path*
    // (rather than a bare name) must use the basename, sanitised to
    // a valid Mighty identifier, as the package name. Pre-fix the
    // generated `src/main.mty` had `package C:/tmp/foo` which is a
    // syntax error the moment you run `mty check`.
    let dir = fresh_tmpdir("webgame-path");
    let target = dir.join("asteroids-pre1");
    let target_str = target.to_string_lossy().into_owned();
    let (code, _stdout, stderr) = mty(
        dir.parent().unwrap_or(&dir),
        &["new", "--template", "web-game", &target_str],
    );
    assert_eq!(code, 0, "stderr={stderr}");
    let main = std::fs::read_to_string(target.join("src/main.mty")).unwrap();
    assert!(
        main.contains("package asteroids_pre1"),
        "main.mty should use sanitised package name `asteroids_pre1`, got:\n{main}"
    );
    assert!(
        !main.contains('/')
            || !main
                .lines()
                .any(|l| l.starts_with("package ") && l.contains('/')),
        "package declaration should not contain a path separator: {main}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_unknown_template_fails_cleanly() {
    let dir = fresh_tmpdir("badtpl");
    let (code, _stdout, stderr) = mty(&dir, &["new", "--template", "no-such-template", "foo"]);
    assert_eq!(code, 2, "stderr={stderr}");
    assert!(
        stderr.to_lowercase().contains("unknown"),
        "expected `unknown` in stderr: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_web_game_check_passes() {
    // `mty check` on the scaffolded src must succeed. This catches
    // any template-side parse / typeck regressions.
    let dir = fresh_tmpdir("webgame-check");
    let (code, _stdout, stderr) = mty(&dir, &["new", "--template", "web-game", "checkme"]);
    assert_eq!(code, 0, "scaffold failed: {stderr}");
    let pkg = dir.join("checkme");
    let (code, stdout, stderr) = mty(&pkg, &["check", "src/main.mty"]);
    assert_eq!(
        code, 0,
        "check failed for scaffolded web-game template:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_web_game_builds_wasm32_web() {
    // Full smoke: scaffold ⇒ build ⇒ artefact on disk.
    let dir = fresh_tmpdir("webgame-build");
    let (code, _stdout, stderr) = mty(&dir, &["new", "--template", "web-game", "buildme"]);
    assert_eq!(code, 0, "scaffold failed: {stderr}");
    let pkg = dir.join("buildme");
    let (code, stdout, stderr) = mty(&pkg, &["build", "--target", "wasm32-web", "src/main.mty"]);
    assert_eq!(
        code, 0,
        "build failed for scaffolded web-game template:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        pkg.join("target").join("main.wasm").is_file(),
        "expected target/main.wasm under {}",
        pkg.display()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
