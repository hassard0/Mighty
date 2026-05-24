# capability_checking

Spec §8 + slice-5 design. Capability narrowing (`fs.ro(...)`,
`net.host(...)`) and call-site subsumption.

## v0.2 conformance gap

The capability subsumption check (SD4010) only fires when a callee
parameter has a *narrower-than-Any* constraint. In v0.2 the surface
syntax always resolves a bare `Fs` / `Net` / `Clock` parameter to
`CapConstraint::Any`; there is no syntax for narrowing a param type
declaration (only the narrowing *constructors* like `fs.ro(p)` exist).

Consequence: SD4010 cannot be triggered from source today. We
populate this category with positive cases — programs that exercise
capability narrowing methods (`fs.ro`, `net.host`) and pass — and
flag the missing negative case in `CONFORMANCE_V0_2_FINDINGS.md`.

## Cases

- `01_cap_param_smoke` — positive: `fn load(fs: Fs)` calls `fs.read(...)`; check is clean.
- `02_cap_narrow_fs_ro` — positive: caller narrows `fs` via `fs.ro("/data")`, binds locally, no SD4010.
- `03_narrow_to_ro` — positive end-to-end: narrowing flows through a function call. (Currently `#[ignore]` per harness — depends on slice-8 cap propagation.)
