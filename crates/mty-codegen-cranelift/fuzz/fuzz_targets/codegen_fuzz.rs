//! Cranelift codegen fuzz target.
//!
//! Goal: when the front-end accepts a program cleanly, lowering it to
//! Cranelift IR + emitting an object file must never panic. Programs
//! that fail parse/type/borrow-check are short-circuited (we leave
//! front-end fuzzing to the parser_fuzz / typeck_fuzz targets).
//!
//! We invoke `compile_object` on a tempfile path rather than `build_jit`
//! because JIT execution would actually *run* the user's `main`, which
//! is out of scope — we only want to exercise the lowering + Cranelift
//! frontend + object emitter.

#![no_main]
use libfuzzer_sys::fuzz_target;
use mty_codegen_cranelift::{compile_object, Monomorphizer};
use mty_diagnostics::Severity;
use mty_driver::pipeline::{lower, parse_source, type_and_borrow_check};

fuzz_target!(|data: &[u8]| {
    let s = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };
    let parsed = parse_source(s.to_string(), "fuzz".into());
    let (pkg, mut diags) = lower(&parsed);
    if diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        return;
    }
    diags.extend(type_and_borrow_check(&pkg));
    if diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        return;
    }
    let typed = mty_types::check_package_typed(&pkg);
    let prog = mty_ir::lower_package(&pkg, &typed);
    let prog = Monomorphizer::new(&prog).run();

    let tmp = match tempfile::Builder::new()
        .prefix("mty-fuzz-")
        .suffix(".o")
        .tempfile()
    {
        Ok(t) => t,
        Err(_) => return,
    };
    let _ = compile_object(&prog, tmp.path());
});
