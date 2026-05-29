//! v0.36 T4 — env-var compatibility shim for the Stardust → Mighty
//! rename.
//!
//! The v0.7 brand rename intentionally retained the `STARDUST_*` env
//! var prefix as a back-compat surface (see `docs/spec/v1.0-rc.md`
//! §2693). This module promotes `MTY_*` to the primary spelling while
//! still honouring `STARDUST_*` — when the legacy name is the only
//! one set, we emit a single-line deprecation warning on stderr and
//! return its value.
//!
//! Precedence:
//!   1. `MTY_<KEY>` if set and non-empty
//!   2. `STARDUST_<KEY>` if set and non-empty (deprecation warning emitted)
//!   3. `None`
//!
//! Use [`lookup_env`] for the common case. Use [`lookup_env_quiet`]
//! when the caller needs to suppress warnings (tests, repeated probes).

use std::sync::atomic::{AtomicBool, Ordering};

/// Process-wide flag so the deprecation warning fires once per
/// (legacy name, process) instead of once per call. Repeated probes
/// (e.g. the runtime checking `MTY_TRACE` at every event) shouldn't
/// produce a wall of stderr.
fn warn_once_flag(key: &str) -> &'static AtomicBool {
    // Hand-rolled tiny intern: we only have ~6 keys, so a linear scan
    // is fine and avoids pulling in once_cell/lazy_static.
    static SLOTS: [(&str, AtomicBool); 16] = [
        ("LINKER", AtomicBool::new(false)),
        ("OTLP_ENDPOINT", AtomicBool::new(false)),
        ("TRACE", AtomicBool::new(false)),
        ("RUNTIME_THREADS", AtomicBool::new(false)),
        ("CONF_ONLY", AtomicBool::new(false)),
        ("CONF_CASE", AtomicBool::new(false)),
        ("HTTP_MOCK", AtomicBool::new(false)),
        ("DET_SEED", AtomicBool::new(false)),
        ("REPLAY_RECORD", AtomicBool::new(false)),
        ("REPLAY_PLAY", AtomicBool::new(false)),
        ("RECORD_TRACE", AtomicBool::new(false)),
        ("_unused_b", AtomicBool::new(false)),
        ("_unused_c", AtomicBool::new(false)),
        ("_unused_d", AtomicBool::new(false)),
        ("_unused_e", AtomicBool::new(false)),
        ("_unused_f", AtomicBool::new(false)),
    ];
    for (k, flag) in SLOTS.iter() {
        if *k == key {
            return flag;
        }
    }
    // Fallback: a shared "other" slot so unknown keys still emit at
    // most once. Should not happen in normal use.
    static FALLBACK: AtomicBool = AtomicBool::new(false);
    &FALLBACK
}

/// Look up an env var by its `MTY_<key>` name, falling back to the
/// legacy `STARDUST_<key>` spelling. When the legacy fallback fires,
/// emit a one-shot deprecation warning on stderr.
///
/// `key` is the suffix (e.g. `"LINKER"` for `MTY_LINKER` /
/// `STARDUST_LINKER`).
pub fn lookup_env(key: &str) -> Option<String> {
    lookup_env_inner(key, /*quiet=*/ false)
}

/// Same as [`lookup_env`] but never emits the deprecation warning.
/// Intended for tests and low-level probes that shouldn't pollute
/// stderr.
pub fn lookup_env_quiet(key: &str) -> Option<String> {
    lookup_env_inner(key, /*quiet=*/ true)
}

fn lookup_env_inner(key: &str, quiet: bool) -> Option<String> {
    let mty_name = format!("MTY_{}", key);
    if let Ok(v) = std::env::var(&mty_name) {
        if !v.is_empty() {
            return Some(v);
        }
    }
    let sd_name = format!("STARDUST_{}", key);
    if let Ok(v) = std::env::var(&sd_name) {
        if !v.is_empty() {
            if !quiet {
                let flag = warn_once_flag(key);
                if !flag.swap(true, Ordering::Relaxed) {
                    eprintln!(
                        "mighty: warning: {} is deprecated; use {} instead",
                        sd_name, mty_name
                    );
                }
            }
            return Some(v);
        }
    }
    None
}

/// Convenience helper for boolean-ish env vars where `"1"`, `"true"`,
/// `"yes"` mean enabled. Treats missing/empty/other as `false`.
pub fn lookup_env_bool(key: &str) -> bool {
    matches!(
        lookup_env(key).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_key() -> String {
        // Use a per-test suffix so concurrent tests don't trample
        // each other's env state.
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        format!("TESTKEY_{}", N.fetch_add(1, Ordering::Relaxed))
    }

    #[test]
    fn mty_takes_precedence_over_stardust() {
        let k = unique_key();
        let mty = format!("MTY_{}", k);
        let sd = format!("STARDUST_{}", k);
        std::env::set_var(&mty, "from-mty");
        std::env::set_var(&sd, "from-stardust");
        assert_eq!(lookup_env_quiet(&k).as_deref(), Some("from-mty"));
        std::env::remove_var(&mty);
        std::env::remove_var(&sd);
    }

    #[test]
    fn stardust_used_when_mty_missing() {
        let k = unique_key();
        let sd = format!("STARDUST_{}", k);
        std::env::set_var(&sd, "legacy-value");
        assert_eq!(lookup_env_quiet(&k).as_deref(), Some("legacy-value"));
        std::env::remove_var(&sd);
    }

    #[test]
    fn empty_string_treated_as_unset_for_both() {
        let k = unique_key();
        let mty = format!("MTY_{}", k);
        let sd = format!("STARDUST_{}", k);
        std::env::set_var(&mty, "");
        std::env::set_var(&sd, "");
        assert!(lookup_env_quiet(&k).is_none());
        std::env::remove_var(&mty);
        std::env::remove_var(&sd);
    }

    #[test]
    fn unset_returns_none() {
        let k = unique_key();
        assert!(lookup_env_quiet(&k).is_none());
    }

    #[test]
    fn lookup_env_bool_parses_truthy() {
        let k = unique_key();
        let mty = format!("MTY_{}", k);
        std::env::set_var(&mty, "1");
        assert!(lookup_env_bool(&k));
        std::env::set_var(&mty, "true");
        assert!(lookup_env_bool(&k));
        std::env::set_var(&mty, "no");
        assert!(!lookup_env_bool(&k));
        std::env::remove_var(&mty);
    }
}
