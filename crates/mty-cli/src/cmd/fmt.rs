use mty_driver::parse_source;
use mty_syntax::SyntaxNode;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

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

/// Outcome of formatting a single input.
enum FormatOutcome {
    /// Source was successfully parsed and a canonical form was produced.
    /// The two strings are (normalized_input, formatted_output).
    Ok { norm: String, out: String },
    /// Input could not be safely formatted. The string explains why.
    /// Caller MUST NOT write to disk when this is returned — that's the
    /// whole point of the v0.42 T5 safety pass (L26 destructive truncation).
    Refused(String),
}

/// Attempt to format `src`. Returns `Refused(reason)` if the formatter
/// cannot proceed safely; otherwise returns the normalized input and the
/// canonical output, both as `\n`-terminated strings.
///
/// The refusal predicates implement v0.42 T5 L26 safety fixes:
/// * Parse must produce zero error diagnostics. The recovery path of the
///   parser is permissive enough that a 100-byte plain-text file produces
///   an *empty* FILE tree (only a recovered WHITESPACE token), so we can't
///   rely on diagnostics alone — but a parse-clean run is a necessary
///   precondition.
/// * The formatted output, when re-parsed, must have the same top-level
///   child-kind sequence as the original. This is the round-trip invariant
///   from `crates/mty-fmt/tests/round_trip.rs`. It catches:
///   - `.txt` truncation: a plain-text input round-trips to an empty FILE,
///     but the original FILE was also empty → caught by the `<same number
///     of items, and that number is > 0 OR the input was already trivial>`
///     guard below.
///   - any future formatter regression that silently drops an item.
fn try_format(norm: &str, source_id: &str) -> FormatOutcome {
    let parsed = parse_source(norm.to_string(), source_id.to_string());
    if !parsed.diagnostics.is_empty() {
        // First parse error wins the message — keep it short for the CLI.
        let first = &parsed.diagnostics[0];
        return FormatOutcome::Refused(format!(
            "parse failed: {} ({})",
            first.primary.message,
            first.code.as_str()
        ));
    }
    let original_kinds: Vec<u16> = SyntaxNode::new_root(parsed.green.clone())
        .children()
        .map(|c| c.kind() as u16)
        .collect();
    let out = mty_fmt::format(parsed.green);

    // Structural-preservation invariant: re-parse the formatter output and
    // require the same top-level child kinds as the input. This is the
    // primary guard against the L26 truncation sharp-edge.
    //
    // The destructive `mty fmt examples/long.txt` reduced a 6480-byte plain
    // text file to 1 byte because the parser's recovery path silently
    // produced an empty FILE tree (no diagnostics, no items). The original
    // input also produced no items, so a naive `original_kinds == new_kinds`
    // comparison would still pass. We therefore additionally require that
    // *if the formatted output is dramatically shorter than the normalized
    // input AND the original tree was empty*, we refuse. This catches the
    // truncation without breaking the (legitimate) "format an empty .mty
    // file" case where input and output are both `\n`.
    let new_parsed = parse_source(out.clone(), source_id.to_string());
    let new_kinds: Vec<u16> = SyntaxNode::new_root(new_parsed.green)
        .children()
        .map(|c| c.kind() as u16)
        .collect();
    if original_kinds != new_kinds {
        return FormatOutcome::Refused(format!(
            "formatter would change top-level item shape ({} item(s) -> {} item(s)); refusing to write",
            original_kinds.len(),
            new_kinds.len()
        ));
    }

    // Empty-tree-with-non-trivial-input guard. If the parser produced an
    // empty FILE but the source clearly carries non-whitespace content,
    // refuse. This is the dedicated catch for the L26 plain-text-file
    // destructive-truncation scenario (the parser recovers a `.txt` file
    // to an empty tree without any error diagnostics).
    if original_kinds.is_empty() {
        let non_ws: usize = norm.chars().filter(|c| !c.is_whitespace()).count();
        if non_ws > 0 {
            return FormatOutcome::Refused(format!(
                "input parsed to an empty Mighty file but contains {non_ws} non-whitespace character(s); refusing to overwrite (input may not be a .mty source)"
            ));
        }
    }

    FormatOutcome::Ok {
        norm: norm.to_string(),
        out,
    }
}

/// Per-path guard: when the caller named an individual file on the
/// command line, require a `.mty` extension. Directory walks already
/// filter to `.mty` via `walk()`, so this only fires on direct file
/// arguments. Prevents the L26 `mty fmt examples/long.txt` truncation
/// at the earliest possible point.
fn is_mty_path(p: &Path) -> bool {
    matches!(p.extension().and_then(|s| s.to_str()), Some("mty"))
}

pub fn run(paths: Vec<PathBuf>, use_stdin: bool, check_only: bool) -> i32 {
    if use_stdin {
        let mut s = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut s) {
            eprintln!("failed to read stdin: {}", e);
            return 1;
        }
        let (norm, _) = normalize_eol(&s);
        match try_format(&norm, "<stdin>") {
            FormatOutcome::Refused(reason) => {
                eprintln!("mty fmt: {}", reason);
                return 1;
            }
            FormatOutcome::Ok { out, .. } => {
                if check_only {
                    return if out == norm { 0 } else { 1 };
                }
                print!("{}", out);
                return 0;
            }
        }
    }
    let mut changed = 0;
    let mut had_refusal = false;
    for path in &paths {
        // For direct file arguments, refuse non-`.mty` paths up front.
        // This is the cheapest, clearest guard against the L26 destructive
        // truncation: `mty fmt foo.txt` now exits non-zero before ever
        // opening the file. Directories are unaffected because `walk()`
        // already filters by `.mty` extension.
        if path.is_file() && !is_mty_path(path) {
            eprintln!(
                "mty fmt: {}: refusing — `mty fmt` only formats `.mty` files",
                path.display()
            );
            had_refusal = true;
            continue;
        }
        for file in collect(path) {
            let src = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("failed to read {}: {}", file.display(), e);
                    return 1;
                }
            };
            let (norm, had_crlf) = normalize_eol(&src);
            let (out, _norm_unused) = match try_format(&norm, &file.display().to_string()) {
                FormatOutcome::Refused(reason) => {
                    eprintln!("mty fmt: {}: {}", file.display(), reason);
                    had_refusal = true;
                    continue;
                }
                FormatOutcome::Ok { out, norm } => (out, norm),
            };
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
    if had_refusal {
        return 1;
    }
    if check_only && changed > 0 {
        1
    } else {
        0
    }
}

fn collect(p: &PathBuf) -> Vec<PathBuf> {
    if p.is_file() {
        // Caller already enforced `.mty` for direct file args (see `run`),
        // so an unguarded include here is safe.
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
