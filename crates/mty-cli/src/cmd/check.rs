use mty_diagnostics::fix::to_ndjson;
use mty_diagnostics::render::ariadne::render_all;
use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source, type_and_borrow_check};
use std::fs;
use std::path::Path;

/// Output mode for `mty check`.
///
/// v0.33 T4 adds the `Json` route: instead of the pretty
/// human-readable report, each diagnostic is emitted as one JSON line
/// (NDJSON) on stdout, conforming to the agent-mode envelope schema
/// documented at `docs/internals/diagnostic-envelopes.md`. With
/// `include_source = true` (`--include-source`) each envelope also
/// carries a 3-line source snippet around the primary span so an
/// agent can render the location without re-reading the file.
#[derive(Debug, Clone, Copy)]
pub enum CheckFormat {
    /// Default — `ariadne` colored report on stderr.
    Pretty,
    /// One JSON envelope per diagnostic, NDJSON-style, on stdout.
    Json,
}

impl CheckFormat {
    pub fn parse(s: &str) -> CheckFormat {
        match s {
            "json" => CheckFormat::Json,
            _ => CheckFormat::Pretty,
        }
    }
}

#[allow(dead_code)]
pub fn run(path: &Path) -> i32 {
    run_with(path, CheckFormat::Pretty, false)
}

pub fn run_with(path: &Path, format: CheckFormat, include_source: bool) -> i32 {
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to read {}: {}", path.display(), e);
            return 1;
        }
    };
    let parsed = parse_source(src.clone(), path.display().to_string());
    let (pkg, mut diags) = lower(&parsed);
    // Run type + borrow check only if lowering produced no hard errors.
    let lower_errors = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !lower_errors {
        diags.extend(type_and_borrow_check(&pkg));
    }
    let has_error = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    match format {
        CheckFormat::Pretty => {
            if !diags.is_empty() {
                eprint!("{}", render_all(&diags, &path.display().to_string(), &src));
            }
            if has_error {
                return 1;
            }
            println!("ok: {}", path.display());
        }
        CheckFormat::Json => {
            // Always emit NDJSON, even on success (an empty result is
            // useful for agents that want to confirm "checked and
            // clean"). One envelope per line on stdout; final newline
            // included so consumers can pipe through `jq -c`.
            let ndjson = to_ndjson(&diags, &path.display().to_string(), &src, include_source);
            print!("{}", ndjson);
            if has_error {
                return 1;
            }
            // No "ok:" line under JSON mode — clean = zero output.
        }
    }
    0
}
