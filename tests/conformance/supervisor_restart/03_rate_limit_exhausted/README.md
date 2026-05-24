# 03 rate_limit_exhausted

Supervisor with `restart up_to 3 in 30s`. The grammar must parse +
type-check. Driving the orchestrator to actually exhaust the rate
limit lives behind the tokio runtime; that path is `#[ignore]` here
and tracked for slice-8. Spec §15.
