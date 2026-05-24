# budget_violation

Spec §16.2 + slice-7 design. Budget blocks (`budget { cpu / wall /
mem / mb } run { ... }`) cap an agent or function's resource use;
exceeding a cap traps with MT5009 (budget_exceeded) or MT5012
(mailbox_full).

## v0.2 conformance gap

The slice-7 SIR interpreter (used by the conformance harness)
applies a single coarse step-budget. Fine-grained `cpu` / `wall`
sub-budgets and mid-turn deadline firing are tokio-runtime-only.

Per amendment A41, deadlines only fire between turns in slice-7.
The wall-budget timeout case is therefore `#[ignore]` in this
harness and tracked for slice-8.

## Cases

- `01_budget_block_smoke` — `budget { cpu / wall / mem / mb } run { ... }` parses + runs trivially.
- `02_step_budget_exceeded` — main computes deeply enough to trip the interp's step budget → exit 3 (MT5009).
- `03_wall_timeout` — deadline-style timeout via `@DUR`; positive-fire path is slice-8 (`#[ignore]`).
- `04_mailbox_full_smoke` — `mb 1k` declaration parses + lowers cleanly.
