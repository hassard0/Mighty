//! `sdust explain <CODE>` — print a human-readable explanation of a
//! diagnostic code from `sdust_diagnostics::codes`.

use sdust_diagnostics::codes;

/// Parse a diagnostic-code argument and print its explanation.
///
/// Accepted formats: `SD0001`, `sd0001`, `0001`, `1`.
///
/// Exit codes:
/// * 0 — code recognized; explanation printed to stdout
/// * 1 — code is well-formed but not a known Stardust diagnostic
/// * 2 — argument is not a valid diagnostic-code string
pub fn run(arg: &str) -> i32 {
    let num_str = arg
        .strip_prefix("SD")
        .or_else(|| arg.strip_prefix("sd"))
        .unwrap_or(arg);
    let n: u16 = match num_str.parse() {
        Ok(n) => n,
        Err(_) => {
            eprintln!(
                "error: expected diagnostic code like `SD0001`, got `{}`",
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
