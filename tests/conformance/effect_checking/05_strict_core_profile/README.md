# 05_strict_core_profile

MT4002 positive-fire (Gap D — v0.10 audit; activated v0.11).

The strict `profile = "core"` rejects the `alloc` effect. The per-case
`mighty.toml` (added v0.11) declares `profile = "core"`; the v0.11
`CwdGuard` mechanism in `conformance_full.rs` chdir's into the case
directory before invoking `check_package_typed`, which then reads
`./mighty.toml` via `mty-types/src/items.rs::load_profile_from_star_toml`
and feeds the Core profile into effect inference.

The pub fn `allocates` uses an `arena { ... }` block, which seeds the
fn's inferred effect set with `alloc`. With Core profile active,
`infer_and_validate` emits MT4002. (MT4001 and MT2001 are emitted
too — alloc is also undeclared, and the arena's tail value type
mismatches Unit — but the case asserts only MT4002 as the targeted
positive-fire and tolerates the others.)

Spec ref: §30 profiles + Gap D recommendation in
`CONFORMANCE_V0_11_NOTES.md`.
