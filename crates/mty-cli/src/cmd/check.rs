use mty_diagnostics::fix::to_ndjson;
use mty_diagnostics::render::ariadne::render_all;
use mty_diagnostics::Severity;
use mty_driver::{
    check_use_resolution, discover_package_sources, find_manifest_root, lower, lower_files,
    parse_source, type_and_borrow_check_with_opts, ParsedFile,
};
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
    // v0.41 T2 — if `path` lives inside a Mighty package, assemble
    // the entire `src/**/*.mty` source set + this file into one HIR
    // `Package` so `use lib.{fn}` resolves against sibling modules.
    // Falls back to the single-file shape for standalone scripts.
    let parsed_target = parse_source(src.clone(), path.display().to_string());
    let mut diags: Vec<mty_diagnostics::Diagnostic> = Vec::new();
    let pkg = if let Some(manifest_dir) = find_manifest_root(path) {
        let src_files = discover_package_sources(&manifest_dir);
        let mut package_modules: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        for p in &src_files {
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                package_modules.insert(stem.to_string());
            }
        }
        let target_id = path.display().to_string();
        let mut all_parsed: Vec<ParsedFile> = src_files
            .iter()
            .filter(|p| p.display().to_string() != target_id)
            .filter_map(|p| std::fs::read_to_string(p).ok().map(|s| (p.clone(), s)))
            .map(|(p, s)| parse_source(s, p.display().to_string()))
            .collect();
        all_parsed.push(parsed_target);
        let (pkg, lower_diags) = lower_files(&all_parsed);
        diags.extend(lower_diags);
        diags.extend(check_use_resolution(&all_parsed, &pkg, &package_modules));
        pkg
    } else {
        let (pkg, lower_diags) = lower(&parsed_target);
        diags.extend(lower_diags);
        pkg
    };
    // Run type + borrow check only if lowering produced no hard errors.
    // v0.42 T6 (L22 fix 3) — `mty check` opts into strict name
    // resolution so an unresolved top-level identifier (e.g.
    // `log(undefined_thing)`) surfaces as MT2021 instead of being
    // silently typed as a fresh inference variable. Other entry points
    // (`mty run`/`mty build`/agent envelopes) keep the historic
    // permissive policy.
    let lower_errors = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !lower_errors {
        let opts = mty_types::items::CheckOpts {
            strict_resolution: true,
        };
        diags.extend(type_and_borrow_check_with_opts(&pkg, opts));
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
