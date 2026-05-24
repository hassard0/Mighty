# 06 cpu_step_budget

v0.3 positive-fire case for the per-handler CPU step budget
(MT5009). Distinct from 02_step_budget_exceeded in that this case
uses a `while` loop (lowers to multi-iteration in slice-6) rather
than `loop` (single-iteration in slice-6). When the v0.3 runtime
applies an additional 1.5x clamp on per-turn CPU time the MT5009 fire
becomes deterministic.

Listed in INTENTIONALLY_IGNORED if the harness sees a 0-exit; tracked
for v0.3 follow-on when `while` lowering pairs with the
RuntimeBuilder default step budget.
