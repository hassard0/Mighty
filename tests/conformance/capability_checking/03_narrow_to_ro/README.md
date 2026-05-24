# 03 narrow_to_ro

Positive end-to-end case: caller narrows `fs` with `fs.ro("/data")`,
then passes the narrowed value to a function that wants an `Fs`.
Currently `#[ignore]` because cap-narrowing propagation across call
sites is slice-8 scope (see `conformance_full.rs` INTENTIONALLY_IGNORED).
Spec §8.1.
