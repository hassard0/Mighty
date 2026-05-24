use mty_driver::parse_source;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

pub fn run(paths: Vec<PathBuf>, use_stdin: bool, check_only: bool) -> i32 {
    if use_stdin {
        let mut s = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut s) {
            eprintln!("failed to read stdin: {}", e);
            return 1;
        }
        let parsed = parse_source(s.clone(), "<stdin>".into());
        let out = mty_fmt::format(parsed.green);
        if check_only {
            return if out == s { 0 } else { 1 };
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
            let parsed = parse_source(src.clone(), file.display().to_string());
            let out = mty_fmt::format(parsed.green);
            if out == src {
                continue;
            }
            if check_only {
                println!("would reformat {}", file.display());
                changed += 1;
            } else {
                if let Err(e) = fs::write(&file, &out) {
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
            } else if p.extension().and_then(|s| s.to_str()) == Some("sd") {
                out.push(p);
            }
        }
    }
}
