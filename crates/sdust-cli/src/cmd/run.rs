use sdust_driver::run_file;
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
    run_file(src, path.display().to_string())
}
