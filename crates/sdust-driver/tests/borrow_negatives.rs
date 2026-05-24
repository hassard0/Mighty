//! Drive every `tests/borrow_neg/*.sd` fixture and assert it produces at
//! least one diagnostic of the expected SD3xxx code (encoded in the
//! filename, e.g. `use_after_move.sd` → SD3001).

use sdust_diagnostics::Severity;
use sdust_driver::{lower, parse_source, type_and_borrow_check};
use std::path::PathBuf;

fn expected_code_for(stem: &str) -> &'static str {
    match stem {
        "use_after_move" => "SD3001",
        "move_out_of_borrow" => "SD3008", // slice 4 simplification: borrowed-then-move
        "borrow_after_move" => "SD3003",
        "mut_borrow_while_shared" => "SD3004",
        "shared_borrow_while_mut" => "SD3005",
        "two_mut_borrows" => "SD3006",
        "cannot_move_borrowed" => "SD3008",
        "move_out_of_ref" => "SD3001", // slice 4 maps to use-after-move shape
        "arena_escape" => "SD3010",
        "non_sendable_message_arg" => "SD3011",
        "mut_borrow_of_immut_local" => "SD3013",
        "assign_to_immut_local" => "SD3014",
        other => panic!("unknown borrow_neg fixture stem: {}", other),
    }
}

fn fixtures_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("tests");
    p.push("borrow_neg");
    p
}

fn check_file(path: &std::path::Path) -> Vec<sdust_diagnostics::Diagnostic> {
    let src = std::fs::read_to_string(path).unwrap();
    let parsed = parse_source(src, path.display().to_string());
    let (pkg, mut diags) = lower(&parsed);
    let lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !lower_err {
        diags.extend(type_and_borrow_check(&pkg));
    }
    diags
}

#[test]
fn borrow_neg_corpus_covers_each_code() {
    let dir = fixtures_dir();
    let mut covered: std::collections::HashSet<String> = Default::default();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("sd") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap()
            .to_string();
        let want = expected_code_for(&stem);
        let diags = check_file(&path);
        let has = diags
            .iter()
            .any(|d| d.code.as_str() == want && matches!(d.severity, Severity::Error));
        assert!(
            has,
            "fixture {}: expected {} but got diagnostics: {:?}",
            stem,
            want,
            diags
                .iter()
                .map(|d| format!("{}={:?}", d.code.as_str(), d.severity))
                .collect::<Vec<_>>()
        );
        covered.insert(want.to_string());
    }
    // Sanity: we covered at least the core SD3xxx codes.
    for code in [
        "SD3001", "SD3003", "SD3004", "SD3005", "SD3006", "SD3008", "SD3010", "SD3011", "SD3013",
        "SD3014",
    ] {
        assert!(covered.contains(code), "no fixture covered {}", code);
    }
}
