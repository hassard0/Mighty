//! Standard macro library bundled with sdust-macros (v0.5).
//!
//! The macros ship as real `.sd` source files in `lib/`. They are
//! `include_str!`d here so the crate can hand callers their source
//! text for compilation as part of a project's macro registry. Real
//! "auto-import on `use sdust_macros.…`" wiring is a sdust-pkg
//! concern — for v0.5 we expose just the text and let projects splice
//! it into their own macro discovery.
//!
//! ## Layout
//!
//!   * `assert.sd` — `assert!`, `assert_eq!`, `assert_ne!`
//!   * `debug.sd`  — `debug!` (eprintln of expression text + value)
//!   * `unreachable.sd` — `unreachable!()`
//!
//! Use [`load_into`] to merge the bundled macros into a
//! [`PackageMacros`] instance.

use crate::registry::PackageMacros;
use sdust_ast::{AstNode, File};
use sdust_syntax::SyntaxNode;

/// Source text of `lib/assert.sd`.
pub const ASSERT_SD: &str = include_str!("../lib/assert.sd");
/// Source text of `lib/debug.sd`.
pub const DEBUG_SD: &str = include_str!("../lib/debug.sd");
/// Source text of `lib/unreachable.sd`.
pub const UNREACHABLE_SD: &str = include_str!("../lib/unreachable.sd");

/// Every bundled source file, in load order.
pub fn bundled_sources() -> &'static [&'static str] {
    &[ASSERT_SD, DEBUG_SD, UNREACHABLE_SD]
}

/// Load every bundled stdlib macro into `pm.local`. Public macros land
/// in `pm.exported` as well so projects can re-export them. Returns
/// the number of macros added.
pub fn load_into(pm: &mut PackageMacros) -> usize {
    let before = pm.local.len();
    for src in bundled_sources() {
        let parsed = sdust_syntax::parse(src);
        let root = SyntaxNode::new_root(parsed.green);
        let Some(file) = File::cast(root) else {
            continue;
        };
        let bundled = PackageMacros::from_file(&file.0);
        // Merge: every bundled macro (public, since the .sd files use
        // `pub macro`) goes into the importer's local + exported set.
        for (name, def) in bundled.local.macros {
            if def.is_pub {
                pm.exported.macros.insert(name.clone(), def.clone());
            }
            pm.local.macros.insert(name, def);
        }
    }
    pm.local.len() - before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assert_sd_parses_and_registers_three_macros() {
        let parsed = sdust_syntax::parse(ASSERT_SD);
        let root = SyntaxNode::new_root(parsed.green);
        let file = File::cast(root).expect("FILE root");
        let pm = PackageMacros::from_file(&file.0);
        assert!(pm.local.contains("assert"), "assert missing");
        assert!(pm.local.contains("assert_eq"), "assert_eq missing");
        assert!(pm.local.contains("assert_ne"), "assert_ne missing");
        assert!(pm.exported.contains("assert"));
        assert!(pm.exported.contains("assert_eq"));
        assert!(pm.exported.contains("assert_ne"));
    }

    #[test]
    fn debug_sd_parses() {
        let parsed = sdust_syntax::parse(DEBUG_SD);
        let root = SyntaxNode::new_root(parsed.green);
        let file = File::cast(root).expect("FILE root");
        let pm = PackageMacros::from_file(&file.0);
        assert!(pm.local.contains("debug"));
    }

    #[test]
    fn unreachable_sd_parses() {
        let parsed = sdust_syntax::parse(UNREACHABLE_SD);
        let root = SyntaxNode::new_root(parsed.green);
        let file = File::cast(root).expect("FILE root");
        let pm = PackageMacros::from_file(&file.0);
        assert!(pm.local.contains("unreachable"));
    }

    #[test]
    fn load_into_merges_all_bundled_macros() {
        let mut pm = PackageMacros::new();
        let added = load_into(&mut pm);
        assert!(added >= 5, "expected >=5 macros, added {added}");
        for name in ["assert", "assert_eq", "assert_ne", "debug", "unreachable"] {
            assert!(pm.local.contains(name), "{name} missing from local");
            assert!(pm.exported.contains(name), "{name} missing from exported");
        }
    }
}
