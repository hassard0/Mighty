# 02 step_budget_exceeded

An unbounded `loop { tick() }` exhausts the interpreter's step
budget (default 1,000,000 steps). The interp reports
`RunResult::BudgetExceeded` → exit code 3 + SD5009. Spec §16.2.
