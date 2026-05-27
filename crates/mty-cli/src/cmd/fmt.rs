use mty_driver::parse_source;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

/// Normalize CRLF → LF before comparing/parsing so that Windows checkouts
/// with `core.autocrlf=true` don't trip `fmt --check` (v0.26 cross-cut fix).
/// Returns (normalized_src, had_crlf).
fn normalize_eol(src: &str) -> (String, bool) {
    if src.contains("\r\n") {
        (src.replace("\r\n", "\n"), true)
    } else {
        (src.to_string(), false)
    }
}

pub fn run(paths: Vec<PathBuf>, use_stdin: bool, check_only: bool) -> i32 {
    if use_stdin {
        let mut s = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut s) {
            eprintln!("failed to read stdin: {}", e);
            return 1;
        }
        let (norm, _) = normalize_eol(&s);
        let parsed = parse_source(norm.clone(), "<stdin>".into());
        let out = mty_fmt::format(parsed.green);
        if check_only {
            return if out == norm { 0 } else { 1 };
        }
        print!("{}", out);
        return 0;
    }
    let mut changed = 0;
    for path in &paths {
        for file in collect(path) {
            let src = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("failed to read {}: {}", file.display(), e);
                    return 1;
                }
            };
            let (norm, had_crlf) = normalize_eol(&src);
            let parsed = parse_source(norm.clone(), file.display().to_string());
            let out = mty_fmt::format(parsed.green);
            if out == norm {
                continue;
            }
            if check_only {
                // Only flag a real fmt drift, not just EOL drift (the file
                // matches the formatter after EOL normalization).
                let _ = had_crlf;
                println!("would reformat {}", file.display());
                changed += 1;
            } else {
                // Preserve the file's original line-ending convention on write.
                let to_write = if had_crlf {
                    out.replace('\n', "\r\n")
                } else {
                    out.clone()
                };
                if let Err(e) = fs::write(&file, &to_write) {
                    eprintln!("failed to write {}: {}", file.display(), e);
                    return 1;
                }
                println!("formatted {}", file.display());
                changed += 1;
            }
        }
    }
    if check_only && changed > 0 {
        1
    } else {
        0
    }
}

fn collect(p: &PathBuf) -> Vec<PathBuf> {
    if p.is_file() {
        vec![p.clone()]
    } else if p.is_dir() {
        let mut out = Vec::new();
        walk(p, &mut out);
        out
    } else {
        Vec::new()
    }
}

fn walk(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("mty") {
                out.push(p);
            }
        }
    }
}
