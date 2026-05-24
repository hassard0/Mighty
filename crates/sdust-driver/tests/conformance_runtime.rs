//! Discover-and-run conformance harness for slice-6 runtime tests.

use sdust_driver::{lower, lower_to_sir, parse_source};
use sdust_sir::interp::{run, BufferHost, RunResult};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

fn run_case(case_dir: &Path) -> Result<(), String> {
    let name = case_dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let input = case_dir.join("input.sd");
    let expected = case_dir.join("expected.txt");
    let src =
        std::fs::read_to_string(&input).map_err(|e| format!("[{}] read input: {}", name, e))?;
    let want = std::fs::read_to_string(&expected)
        .map_err(|e| format!("[{}] read expected: {}", name, e))?;

    let parsed = parse_source(src, input.display().to_string());
    let (pkg, _) = lower(&parsed);
    let (prog, _) = lower_to_sir(&pkg);
    let mut host = BufferHost::default();
    let res = run(&prog, &mut host);
    let got = host.stdout_str();

    if want.trim() == "__TRAP__" {
        if matches!(res, RunResult::Trap { .. }) {
            return Ok(());
        }
        return Err(format!(
            "[{}] expected __TRAP__, got {:?}\nstdout: {:?}",
            name, res, got
        ));
    }

    // Permissive newline handling: compare with trim_end on both sides
    // (so editors don't matter).
    let want_norm = want.replace("\r\n", "\n");
    let got_norm = got.replace("\r\n", "\n");
    if want_norm.trim_end() != got_norm.trim_end() {
        return Err(format!(
            "[{}] stdout mismatch\nwant: {:?}\ngot:  {:?}",
            name, want_norm, got_norm
        ));
    }
    Ok(())
}

#[test]
fn runtime_conformance_corpus() {
    let root = workspace_root().join("tests/conformance/runtime");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("read conformance/runtime dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    entries.sort();

    let mut failures: Vec<String> = vec![];
    let mut count = 0;
    for dir in entries {
        // Skip dirs without input.sd (the legacy parser/* etc. dirs).
        if !dir.join("input.sd").exists() {
            continue;
        }
        count += 1;
        if let Err(e) = run_case(&dir) {
            failures.push(e);
        }
    }

    assert!(count >= 5, "expected ≥5 conformance cases, found {}", count);
    assert!(
        failures.is_empty(),
        "{} runtime conformance failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
