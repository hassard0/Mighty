# 08 — Supervisors

> **Status:** supervisor strategies, restart rate limits
> (`restart up_to N in DUR`), and uniform-jitter backoff
> (`backoff D1..D2`) are implemented at the runtime layer as of
> v0.7. See
> [`docs/internals/supervisors.md`](../internals/supervisors.md).

A supervisor owns a set of child agents and decides what to do when
one fails. Mighty's supervisors borrow from Erlang/OTP: strategies
are named, and each child gets an explicit restart policy.

## The program

```mty
supervisor SearchFlow(strategy: one_for_one) {
  child planner = spawn Planner()
  child fetcher = spawn Fetcher(net)

  on_fail(planner) { restart up_to 3 in 30s }
  on_fail(fetcher) { backoff 100ms..2s; restart }
}
```

## What is interesting

- `supervisor SearchFlow(strategy: one_for_one)` names the supervisor
  and selects its strategy. Per [spec §15](../spec/v1.0-rc.md), valid
  strategies are `one_for_one`, `one_for_all`, `rest_for_one`, and
  `escalate`.
- `child planner = spawn Planner()` declares a child agent. The
  handle `planner` is in scope inside `on_fail` clauses.
- `on_fail(planner) { restart up_to 3 in 30s }` says: if `planner`
  fails, restart it up to 3 times in any 30-second window. Exceeding
  the budget triggers `MT5014 restart_limit_exceeded` and escalation
  per the strategy.
- `on_fail(fetcher) { backoff 100ms..2s; restart }` adds uniformly
  distributed jittered backoff between 100ms and 2s before
  restarting.

## Try it

```bash
mty check examples/10_supervisor.mty
```

## Next

Continue to [09 — Arenas](09-arenas.md).
