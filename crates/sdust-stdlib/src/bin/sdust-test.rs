//! `sdust-test` — Stardust test-runner CLI.
//!
//! Walks `tests/` (or `--dir <path>`) in the current package, runs
//! every `fn test_*` it finds, and prints a `cargo test`-style report.
//! Exit code: 0 on all-pass, 1 on any failure.
//!
//! v0.3 plan: merge this into `sdust test` as a subcommand of the main
//! `sdust` CLI. We ship it as a standalone binary in v0.2 to respect
//! the wave-2 work-area constraints documented in `STDLIB_V0_2_NOTES.md`.

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut dir: PathBuf = PathBuf::from("tests");
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dir" => {
                if let Some(d) = args.next() {
                    dir = PathBuf::from(d);
                }
            }
            "--help" | "-h" => {
                println!("sdust-test — run Stardust tests");
                println!();
                println!("Usage: sdust-test [--dir <path>]");
                println!();
                println!("Default --dir is `tests/`. Every fn whose name starts with");
                println!("`test_` in every .sd file under that directory is invoked.");
                return;
            }
            _ => {}
        }
    }
    let summary = sdust_stdlib::test::run_dir(&dir);
    print!("{}", summary.output);
    std::process::exit(summary.exit_code());
}
