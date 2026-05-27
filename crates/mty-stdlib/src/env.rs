//! `std.env` — process-environment surface for Mighty source.
//!
//! v0.27 Track E (QoL gap #3): `mty run <path> -- <argv>` now forwards
//! its positional tail into Mighty source as `std.env.args()`. The CLI
//! captures the trailing args after `--` and stashes them in the
//! process-wide [`ARGS`] channel before invoking the runtime; the
//! `std.env.args` dispatch arm reads from this channel and hands the
//! agent a `Value::Array(Vec<Value::Str>)`.
//!
//! Surface (Mighty side):
//!
//! ```ignore
//! let argv = std.env.args()       // -> List[Str]
//! let q = argv.get(0)              // -> Option[Str]
//! ```
//!
//! Convention chosen: the leading positional (after `--`) is index 0,
//! matching `std::env::args().skip(1)` semantics from a user
//! perspective. Mighty source treats `std.env.args()` as "the args this
//! Mighty program received," not "the OS-level argv-with-binary."

use std::sync::OnceLock;
use std::sync::RwLock;

/// Process-wide channel for the argv tail the CLI captured after `--`.
///
/// `set_args` is called once from `mty-cli`'s `Run` dispatch before
/// the runtime spins up. `args` reads the channel; an empty `Vec` is
/// returned if no `set_args` ever fired (e.g. JIT builds, library
/// callers, the wasm32-wasi backend — they all see "no extra argv").
static ARGS: OnceLock<RwLock<Vec<String>>> = OnceLock::new();

fn cell() -> &'static RwLock<Vec<String>> {
    ARGS.get_or_init(|| RwLock::new(Vec::new()))
}

/// Install the argv tail. Called from `mty-cli`'s `Run` dispatch with
/// whatever followed `--` on the command line. Idempotent — last write
/// wins, which matches what tests want (each test installs its own).
pub fn set_args(args: Vec<String>) {
    if let Ok(mut guard) = cell().write() {
        *guard = args;
    }
}

/// Snapshot the installed argv tail. Returns an empty `Vec` when
/// nothing has been installed.
pub fn args() -> Vec<String> {
    cell().read().map(|g| g.clone()).unwrap_or_default()
}

/// Test-only: clear the argv tail. Lets the test suite simulate "no
/// args were passed" without interfering with sibling tests that did
/// install args.
#[doc(hidden)]
pub fn reset_for_tests() {
    if let Ok(mut guard) = cell().write() {
        guard.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_starts_empty() {
        reset_for_tests();
        assert!(args().is_empty());
    }

    #[test]
    fn set_args_round_trips() {
        reset_for_tests();
        set_args(vec!["hello".into(), "world".into()]);
        let got = args();
        assert_eq!(got, vec!["hello".to_string(), "world".to_string()]);
    }

    #[test]
    fn set_args_overwrites() {
        reset_for_tests();
        set_args(vec!["a".into()]);
        set_args(vec!["b".into(), "c".into()]);
        assert_eq!(args(), vec!["b".to_string(), "c".to_string()]);
    }
}
