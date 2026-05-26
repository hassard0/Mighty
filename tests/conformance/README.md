# Mighty Conformance Suite

Per Mighty v0.1 spec §37 (now §37 of v1.0-RC2). Each subdirectory holds
tests for one category. The current corpus has **140 cases across 24
populated categories** — every category is populated as of v0.20.

As of v0.19 the conformance suite is also published as a downloadable
**kit** for use by independent implementations — see
`CONFORMANCE_KIT.md` in this directory and `docs/spec/conformance.md`
for the normative description of how to use it. The kit is built by
`scripts/build-conformance-kit.sh` and (since v0.20) is attached to
every tagged GitHub Release by `.github/workflows/release.yml`.

The machine-readable diagnostic-code coverage report lives in
`coverage.json` — it lists every registered `MTxxxx` code split into
`covered` / `auxiliary` / `uncovered` tiers.

## Categories (v0.20)

* `lexical/` (3), `parser/` (1)
* `type_checking/` (20), `type_inference/` (5)
* `borrow_checking/` (13), `ownership_rejection/` (4)
* `effect_checking/` (5), `capability_checking/` (4)
* `budget_violation/` (6), `traits_derive/` (4)
* `macros/` (6)
* `agent_protocol/` (5), `mailbox_ordering/` (7), `supervisor_restart/` (4)
* `control_flow/` (5)
* `runtime/` (6), `runtime-7/` (8), `runtime_traps/` (2)
* `codegen/` (9)
* `spec_coverage/` (5)
* **v0.20 newly populated:** `deterministic_replay/` (5),
  `formatter_idempotence/` (5), `native_abi/` (4),
  `wasm_component/` (4)

## Running

Rust reference (inside the workspace):

```
cargo test -p mty-syntax --test parse_recovery
cargo test -p mty-fmt --test idempotence
cargo test -p mty-fmt --test round_trip
cargo test --workspace                       # full sweep
```

Out-of-tree (kit consumers): see the test-driver protocol in
`docs/spec/conformance.md`.

## Building the kit tarball

```
scripts/build-conformance-kit.sh                       # uses git describe for version
scripts/build-conformance-kit.sh v1.0-rc1              # explicit version
```

The output `mty-conformance-kit-<version>.tar.gz` lands in the repo
root, ready to upload to a GitHub release.

## Adding a test

1. Drop the input `.mty` file in the appropriate category in a new
   `NN_short_name/input.mty` directory.
2. Write `expected_diagnostics.txt` (one diagnostic per line; codes
   first, then optional message tail) and `expected_exit_code.txt`.
3. Write `command.txt` with the suggested CLI invocation
   (e.g., `mty check input.mty`).
4. Add a Rust test that loads it and asserts the expected outcome
   (parse OK, specific diagnostic, fmt idempotence, etc.).
5. If the new case requires a new diagnostic code, register the code
   in `crates/mty-types`'s diagnostic registry and update
   `docs/reference/diagnostics.md`.
