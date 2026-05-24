use sdust_diagnostics::render::ariadne::render_all;
use sdust_driver::{lower, parse_source};
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
    let (_pkg, diags) = lower(&parsed);
    if !diags.is_empty() {
        eprint!("{}", render_all(&diags, &path.display().to_string(), &src));
        return 1;
    }
    println!("ok: {}", path.display());
    0
}
