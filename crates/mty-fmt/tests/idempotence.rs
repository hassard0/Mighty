use mty_syntax::parse;
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
fn fmt_preserves_non_trivia_token_stream() {
    use mty_syntax::{lex, SyntaxKind};
    let files = collect_sd_files();
    let mut failed = Vec::new();
    for path in files {
        let src = fs::read_to_string(&path).unwrap();
        let formatted = mty_fmt::format(mty_syntax::parse(&src).green);
        let orig: Vec<(SyntaxKind, String)> = lex(&src)
            .into_iter()
            .filter(|t| !t.kind.is_trivia() && t.kind != SyntaxKind::EOF)
            .map(|t| (t.kind, t.text.to_string()))
            .collect();
        let new_: Vec<(SyntaxKind, String)> = lex(&formatted)
            .into_iter()
            .filter(|t| !t.kind.is_trivia() && t.kind != SyntaxKind::EOF)
            .map(|t| (t.kind, t.text.to_string()))
            .collect();
        if orig != new_ {
            failed.push(format!(
                "{}: non-trivia token stream changed (orig {} tokens, new {} tokens)",
                path.display(),
                orig.len(),
                new_.len(),
            ));
        }
    }
    assert!(
        failed.is_empty(),
        "{} files: {}",
        failed.len(),
        failed.join("\n")
    );
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
        let once = mty_fmt::format(parse(&src).green);
        let twice = mty_fmt::format(parse(&once).green);
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
