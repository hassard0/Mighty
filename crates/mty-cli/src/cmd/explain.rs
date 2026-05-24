//! `mty explain <CODE>` — print a human-readable explanation of a
//! diagnostic code from `mty_diagnostics::codes`.

use mty_diagnostics::codes;

/// Parse a diagnostic-code argument and print its explanation.
///
/// Accepted formats: `MT0001`, `mt0001`, `SD0001` (legacy), `sd0001`
/// (legacy), `0001`, `1`. The `SD`/`sd` prefixes are kept for
/// back-compat with v0.6 docs and bug reports filed pre-rebrand.
///
/// Exit codes:
/// * 0 — code recognized; explanation printed to stdout
/// * 1 — code is well-formed but not a known Mighty diagnostic
/// * 2 — argument is not a valid diagnostic-code string
pub fn run(arg: &str) -> i32 {
    let num_str = arg
        .strip_prefix("MT")
        .or_else(|| arg.strip_prefix("mt"))
        .or_else(|| arg.strip_prefix("SD"))
        .or_else(|| arg.strip_prefix("sd"))
        .unwrap_or(arg);
    let n: u16 = match num_str.parse() {
        Ok(n) => n,
        Err(_) => {
            eprintln!(
                "error: expected diagnostic code like `MT0001`, got `{}`",
                arg
            );
            return 2;
        }
    };
    let code = codes::DiagCode::new(n);
    match codes::explain(code) {
        Some(text) => {
            println!("{}", text);
            0
        }
        None => {
            eprintln!("error: unknown diagnostic code {}", code.as_str());
            1
        }
    }
}
