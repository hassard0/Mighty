use sdust_driver::{run_file, run_file_with_runtime};
use std::fs;
use std::path::Path;

pub fn run(path: &Path, legacy: bool) -> i32 {
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to read {}: {}", path.display(), e);
            return 1;
        }
    };
    let id = path.display().to_string();
    if legacy {
        run_file(src, id)
    } else {
        run_file_with_runtime(src, id)
    }
}
