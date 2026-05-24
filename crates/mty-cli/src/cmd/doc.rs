//! `sdust doc` — Mighty documentation generator. See
//! `docs/reference/cli/mty-doc.md` for the user-facing reference and
//! `docs/internals/doc-generator.md` for the algorithm.

use std::path::{Path, PathBuf};

pub fn run(
    path: &Path,
    item: Option<String>,
    html: bool,
    markdown: bool,
    out_dir: Option<PathBuf>,
    check_examples: bool,
) -> i32 {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("sdust doc: {}: {}", path.display(), e);
            return 1;
        }
    };
    let default_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("package")
        .to_string();
    let (doc, diags) = mty_doc::build_doc_package(&src, &path.to_string_lossy(), &default_name);
    let any_err = diags
        .iter()
        .any(|d| matches!(d.severity, mty_diagnostics::Severity::Error));
    if any_err {
        use mty_diagnostics::render::ariadne::render_all;
        eprint!("{}", render_all(&diags, &path.to_string_lossy(), &src));
        return 1;
    }

    if check_examples {
        // v0.2: examples are extracted but not type-checked; surface a
        // note instead of silently doing nothing.
        eprintln!(
            "sdust doc: --check-examples is a no-op in v0.2 (extraction-only); see DOC_V0_2_NOTES.md"
        );
    }

    if html {
        let dir = out_dir.unwrap_or_else(|| PathBuf::from("target/doc").join(&doc.name));
        let files = mty_doc::render::html(&doc);
        if let Err(e) = mty_doc::render::write_tree(&dir, &files) {
            eprintln!("sdust doc: write {}: {}", dir.display(), e);
            return 1;
        }
        println!("wrote {} files to {}", files.len(), dir.display());
        return 0;
    }
    if markdown {
        let dir = out_dir.unwrap_or_else(|| PathBuf::from("target/doc-md").join(&doc.name));
        let files = mty_doc::render::markdown(&doc);
        if let Err(e) = mty_doc::render::write_tree(&dir, &files) {
            eprintln!("sdust doc: write {}: {}", dir.display(), e);
            return 1;
        }
        println!("wrote {} files to {}", files.len(), dir.display());
        return 0;
    }

    // Plain stdout mode.
    if let Some(name) = item {
        if let Some(it) = doc.items.iter().find(|i| i.name == name) {
            print!("{}", mty_doc::render::item_text(&doc, it));
            0
        } else {
            eprintln!(
                "sdust doc: no item named `{}` in package `{}`",
                name, doc.name
            );
            1
        }
    } else {
        print!("{}", mty_doc::render::text(&doc));
        0
    }
}
