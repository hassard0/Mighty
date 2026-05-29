//! `mty doc` — Mighty documentation generator. See
//! `docs/reference/cli/mty-doc.md` for the user-facing reference and
//! `docs/internals/doc-generator.md` for the algorithm.
//!
//! v0.35 T5 added [`run_check`] — the stdlib-hover-catalog drift gate.
//! See `docs/internals/stdlib-docs-pipeline.md`.

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
            eprintln!("mty doc: {}: {}", path.display(), e);
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
            "mty doc: --check-examples is a no-op in v0.2 (extraction-only); see DOC_V0_2_NOTES.md"
        );
    }

    if html {
        let dir = out_dir.unwrap_or_else(|| PathBuf::from("target/doc").join(&doc.name));
        let files = mty_doc::render::html(&doc);
        if let Err(e) = mty_doc::render::write_tree(&dir, &files) {
            eprintln!("mty doc: write {}: {}", dir.display(), e);
            return 1;
        }
        println!("wrote {} files to {}", files.len(), dir.display());
        return 0;
    }
    if markdown {
        let dir = out_dir.unwrap_or_else(|| PathBuf::from("target/doc-md").join(&doc.name));
        let files = mty_doc::render::markdown(&doc);
        if let Err(e) = mty_doc::render::write_tree(&dir, &files) {
            eprintln!("mty doc: write {}: {}", dir.display(), e);
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
                "mty doc: no item named `{}` in package `{}`",
                name, doc.name
            );
            1
        }
    } else {
        print!("{}", mty_doc::render::text(&doc));
        0
    }
}

/// `mty doc --check` — the v0.35 T5 Strategy B drift gate.
///
/// Builds the extracted stdlib catalog from the per-module docstubs
/// at `crates/mty-stdlib/docs/*.docstub` (compile-time embedded), then
/// compares it to the curated [`mty_doc::STDLIB_EXAMPLES`] gold-set.
/// Emits a stable Markdown report and returns:
///
/// - `0` on zero-drift (extracted ≡ curated).
/// - `1` on any divergence (missing entry, extra entry, or field
///   mismatch on a shared symbol).
///
/// When `report_path` is set, the same payload is written to that
/// file in addition to stdout — handy for CI artefact uploads.
pub fn run_check(report_path: Option<&Path>) -> i32 {
    let extracted = mty_doc::build_extracted_catalog();
    let drift = mty_doc::diff_catalogs(&extracted, mty_doc::STDLIB_EXAMPLES);

    let mut payload = String::new();
    payload.push_str(&format!(
        "mty doc check (v0.35 T5 Strategy B drift gate)\n\
         curated entries:   {}\n\
         extracted entries: {}\n\n",
        mty_doc::STDLIB_EXAMPLES.len(),
        extracted.len(),
    ));
    if drift.is_empty() {
        payload.push_str("OK: extracted catalog matches curated table byte-for-byte.\n");
        print!("{payload}");
        if let Some(p) = report_path {
            if let Err(e) = std::fs::write(p, &payload) {
                eprintln!("mty doc check: write {}: {}", p.display(), e);
            }
        }
        0
    } else {
        payload.push_str(&mty_doc::render_drift_report(&drift));
        payload.push_str(
            "\nTo fix: edit `crates/mty-stdlib/docs/<module>.docstub` to \
             match the curated table, OR regenerate it from the curated \
             gold-set via `cargo run -p mty-doc --bin regen-stdlib-docstubs`.\n",
        );
        print!("{payload}");
        if let Some(p) = report_path {
            if let Err(e) = std::fs::write(p, &payload) {
                eprintln!("mty doc check: write {}: {}", p.display(), e);
            }
        }
        1
    }
}
