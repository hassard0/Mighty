use sdust_syntax::parse;
use std::fs;
use std::path::PathBuf;

fn collect_sd_files() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for dir in [root.join("examples"), root.join("tests/fmt/fixtures")] {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir).unwrap() {
            let p = entry.unwrap().path();
            if p.extension().and_then(|s| s.to_str()) == Some("sd") {
                out.push(p);
            }
        }
    }
    out
}

#[test]
fn fmt_is_idempotent() {
    let files = collect_sd_files();
    assert!(
        !files.is_empty(),
        "no .sd files found — expected at least 00_smoke.sd"
    );
    let mut failed = Vec::new();
    for path in files {
        let src = fs::read_to_string(&path).unwrap();
        let once = sdust_fmt::format(parse(&src).green);
        let twice = sdust_fmt::format(parse(&once).green);
        if once != twice {
            failed.push(format!(
                "{}:\n--- once ---\n{}\n--- twice ---\n{}",
                path.display(),
                once,
                twice
            ));
        }
    }
    assert!(
        failed.is_empty(),
        "{} files not idempotent:\n{}",
        failed.len(),
        failed.join("\n\n")
    );
}
