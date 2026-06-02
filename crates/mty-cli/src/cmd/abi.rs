//! v0.46 T1 — `mty abi`: inspect the runtime ABI surface.
//!
//! The Mighty compiler emits calls into a stable family of
//! `mty_runtime_*` C-ABI symbols. v0.46 T1 ships an official artifact
//! pipeline (generated header + staticlib in release.yml +
//! check-in `crates/mty-runtime/include/mty_runtime_abi.h`); this
//! subcommand gives humans and CI / IDE tooling a fast, dependency-
//! free way to verify against the ground truth at runtime.
//!
//! ```text
//! mty abi list             # default plain text, one line per symbol
//! mty abi list --format json
//! mty abi version
//! mty abi header           # print the canonical C header to stdout
//! ```
//!
//! See `docs/internals/runtime-abi.md` for the consumer side.

use mty_runtime::abi_export::{
    signatures_json, signatures_text, RUNTIME_ABI_HEADER, RUNTIME_ABI_SIGNATURES,
    RUNTIME_ABI_VERSION,
};

/// The three `mty abi` sub-commands. Kept in this module instead of
/// `main.rs` so the dispatch table stays close to the implementations.
#[derive(Debug, Clone)]
pub enum AbiCmd {
    /// Dump the runtime ABI symbol list.
    List { format: ListFormat },
    /// Print just the ABI version (matches `MTY_RUNTIME_ABI_VERSION`).
    Version,
    /// Print the canonical C header to stdout.
    Header,
}

#[derive(Debug, Clone, Copy)]
pub enum ListFormat {
    Plain,
    Json,
}

impl ListFormat {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "plain" | "text" => Some(Self::Plain),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

pub fn run(cmd: AbiCmd) -> i32 {
    match cmd {
        AbiCmd::List { format } => match format {
            ListFormat::Plain => {
                print!("{}", signatures_text());
                println!(
                    "# {} symbols, version {}",
                    RUNTIME_ABI_SIGNATURES.len(),
                    RUNTIME_ABI_VERSION
                );
                0
            }
            ListFormat::Json => {
                print!("{}", signatures_json());
                0
            }
        },
        AbiCmd::Version => {
            println!("{}", RUNTIME_ABI_VERSION);
            0
        }
        AbiCmd::Header => {
            print!("{}", RUNTIME_ABI_HEADER);
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_format_parses_known_values() {
        assert!(matches!(
            ListFormat::parse("plain"),
            Some(ListFormat::Plain)
        ));
        assert!(matches!(ListFormat::parse("text"), Some(ListFormat::Plain)));
        assert!(matches!(ListFormat::parse("json"), Some(ListFormat::Json)));
        assert!(ListFormat::parse("yaml").is_none());
    }

    #[test]
    fn version_matches_runtime_constant() {
        // Smoke check that we re-export through this module.
        assert!(!RUNTIME_ABI_VERSION.is_empty());
    }

    #[test]
    fn header_contains_version_guard() {
        assert!(RUNTIME_ABI_HEADER.contains("#define MTY_RUNTIME_ABI_VERSION"));
        assert!(RUNTIME_ABI_HEADER.contains("MTY_RUNTIME_ABI_H"));
    }
}
