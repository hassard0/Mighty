# supervisor_restart

Spec §15 + slice-7 design. Supervisors observe child failure and
react per strategy:

- `one_for_one` — restart only the failed child.
- `one_for_all` — restart all siblings.
- `escalate` — propagate failure up.
- `restart up_to N in DUR` — rate-limit; if exhausted, escalate.

## v0.2 conformance gap

The slice-7 runtime declares supervisors and registers them with the
orchestrator, but the conformance harness routes `run` through the
slice-6 SIR interpreter (deterministic + fast). The interp accepts
supervisor declarations and runs `main`; actual restart sequencing
happens in the tokio runtime, exercised by `crates/sdust-runtime/tests`.

These cases lock in the **declaration grammar + lowering**: every
strategy must parse, type-check, and run a trivial main cleanly.
The `03_rate_limit_exhausted` case (which requires the orchestrator
to fire a child failure) is `#[ignore]`d per harness, documented in
findings, and re-enabled in slice-8.

## Cases

- `01_one_for_one` — `supervisor S(strategy: one_for_one) { ... }` parses + runs.
- `02_escalate` — declared `escalate` policy parses + runs.
- `03_rate_limit_exhausted` — `restart up_to 3 in 30s` declaration parses; runtime exhaustion path is slice-8 (`#[ignore]`).
- `04_two_children` — supervisor with two children + per-child on_fail clauses.
