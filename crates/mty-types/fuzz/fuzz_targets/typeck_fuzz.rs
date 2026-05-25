//! Type-checker fuzz target.
//!
//! Goal: the full parse → HIR-lower → type-check pipeline must never
//! panic on arbitrary UTF-8 input. The type checker runs as part of
//! `mty_driver::pipeline::lower` (HIR lowering) plus the explicit
//! `type_check` call. We exercise the typed path via
//! `check_package_typed` so the side tables get built too.

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let parsed = mty_driver::pipeline::parse_source(s.to_string(), "fuzz".into());
        let (pkg, _diags) = mty_driver::pipeline::lower(&parsed);
        // Force the typed path: builds DefMap + TyArena + per-expr
        // resolved types. This is the broadest type-checker call.
        let _typed = mty_types::check_package_typed(&pkg);
    }
});
