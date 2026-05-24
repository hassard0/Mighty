use sdust_diagnostics::render::ariadne::render_all;
use sdust_diagnostics::Severity;
use sdust_driver::{lower, parse_source, type_check};
use std::fs;
use std::path::Path;

pub fn run(path: &Path) -> i32 {
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to read {}: {}", path.display(), e);
            return 1;
        }
    };
    let parsed = parse_source(src.clone(), path.display().to_string());
    let (pkg, mut diags) = lower(&parsed);
    // Only run the type checker if lowering produced no hard errors.
    let lower_errors = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !lower_errors {
        diags.extend(type_check(&pkg));
    }
    let has_error = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !diags.is_empty() {
        eprint!("{}", render_all(&diags, &path.display().to_string(), &src));
    }
    if has_error {
        return 1;
    }
    println!("ok: {}", path.display());
    0
}
