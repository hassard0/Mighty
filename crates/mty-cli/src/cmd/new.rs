use std::fs;
use std::path::Path;

pub fn run(name: &str) -> i32 {
    let dir = Path::new(name);
    if dir.exists() {
        eprintln!("directory `{}` already exists", name);
        return 1;
    }
    if let Err(e) = fs::create_dir_all(dir.join("src")) {
        eprintln!("failed to create directory: {}", e);
        return 1;
    }
    let manifest = format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2026"
profile = "host"

[deps]
"#,
        name
    );
    if let Err(e) = fs::write(dir.join("mighty.toml"), manifest) {
        eprintln!("failed to write mighty.toml: {}", e);
        return 1;
    }
    if let Err(e) = fs::write(
        dir.join("src").join("main.mty"),
        "fn main() {\n  log(\"hello, Mighty\")\n}\n",
    ) {
        eprintln!("failed to write src/main.mty: {}", e);
        return 1;
    }
    println!("created {}/", name);
    0
}
