//! `std.time` — monotonic clock + duration arithmetic.
//!
//! Wraps `std::time::Instant` (monotonic on all supported platforms) and
//! `std::time::Duration`. A `Clock` capability is required to call
//! `now` / `sleep`; the runtime synthesizes one per agent based on the
//! manifest's `time` grant.
//!
//! ## Backend dispatch (v0.14 P2 lowering)
//!
//! When a program is compiled with `--wasi=p2`, the Mighty codegen
//! lowers these calls directly to versioned P2 imports rather than
//! routing through the `wasi_snapshot_preview1` adapter. The
//! canonical import shapes are exposed below as
//! [`P2_DIRECT_IMPORT_MONOTONIC_NOW`] /
//! [`P2_DIRECT_IMPORT_WALL_CLOCK_NOW`] /
//! [`P2_DIRECT_IMPORT_MONOTONIC_RESOLUTION`] — these match the
//! variants of `mty_codegen_wasm::P2DirectImport` and are pinned
//! here so the stdlib and codegen layers never drift on naming.
//!
//! The native runtime path is unchanged — the import-shape switch
//! is purely a Wasm-side concern.

use std::time::{Duration, Instant as StdInstant};

/// Canonical P2 import name for the monotonic-clock `now()` call.
/// See module doc for the v0.14 dispatch rationale.
pub const P2_DIRECT_IMPORT_MONOTONIC_NOW: (&str, &str) =
    ("wasi:clocks/monotonic-clock@0.2.3", "now");

/// Canonical P2 import name for the wall-clock `now()` call.
pub const P2_DIRECT_IMPORT_WALL_CLOCK_NOW: (&str, &str) = ("wasi:clocks/wall-clock@0.2.3", "now");

/// Canonical P2 import name for the monotonic-clock `resolution()`
/// call.
pub const P2_DIRECT_IMPORT_MONOTONIC_RESOLUTION: (&str, &str) =
    ("wasi:clocks/monotonic-clock@0.2.3", "resolution");

#[derive(Debug, Clone, Copy)]
pub struct Clock;

#[derive(Debug, Clone, Copy)]
pub struct Instant(pub StdInstant);

impl Instant {
    /// Time elapsed between `other` and `self`. Returns `Duration::ZERO`
    /// when `self < other` (matches std's `checked_duration_since`).
    pub fn elapsed_since(self, other: Instant) -> Duration {
        self.0.checked_duration_since(other.0).unwrap_or_default()
    }
}

pub fn now(_cap: Clock) -> Instant {
    Instant(StdInstant::now())
}

pub async fn sleep(_cap: Clock, dur: Duration) {
    tokio::time::sleep(dur).await;
}

/// Synchronous (blocking) sleep — used by the JIT/interpreter when no
/// tokio context is available.
pub fn sleep_blocking(_cap: Clock, dur: Duration) {
    std::thread::sleep(dur);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_monotonic() {
        let a = now(Clock);
        std::thread::sleep(Duration::from_millis(1));
        let b = now(Clock);
        assert!(b.elapsed_since(a) >= Duration::from_millis(1));
    }

    #[test]
    fn p2_direct_import_constants_are_canonical() {
        // Pin the import shapes so a regression in either the
        // codegen layer or this stdlib doesn't drift them apart.
        assert_eq!(
            P2_DIRECT_IMPORT_MONOTONIC_NOW,
            ("wasi:clocks/monotonic-clock@0.2.3", "now")
        );
        assert_eq!(
            P2_DIRECT_IMPORT_WALL_CLOCK_NOW,
            ("wasi:clocks/wall-clock@0.2.3", "now")
        );
        assert_eq!(
            P2_DIRECT_IMPORT_MONOTONIC_RESOLUTION,
            ("wasi:clocks/monotonic-clock@0.2.3", "resolution")
        );
    }

    #[test]
    fn elapsed_handles_reversed() {
        let a = now(Clock);
        std::thread::sleep(Duration::from_millis(1));
        let b = now(Clock);
        // a < b → a.elapsed_since(b) saturates to zero.
        assert_eq!(a.elapsed_since(b), Duration::ZERO);
    }
}
