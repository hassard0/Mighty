# Supervisors (slice 7)

**Module:** `sdust_runtime::supervisor`
**Spec:** §15 Supervisors

## Strategies

```rust
pub enum Strategy {
    OneForOne = 0,
    OneForAll = 1,
    RestForOne = 2,
    Escalate = 3,
}
```

Spec §15 table:

| Strategy        | Meaning                                              |
|-----------------|------------------------------------------------------|
| `one_for_one`   | restart only the failed child                        |
| `one_for_all`   | restart all children                                 |
| `rest_for_one`  | restart failed child and children declared after it  |
| `escalate`      | propagate failure to the parent supervisor           |

## Restart rate limit + backoff

`restart up_to N in DUR` and `backoff D1..D2` from spec §15 are
encoded in `RestartPolicy`:

```rust
pub struct RestartPolicy {
    pub max_attempts: u32,    // N
    pub window: Duration,     // DUR
    pub backoff_min: Duration,
    pub backoff_max: Duration,
}
```

`RestartTracker::may_restart()` returns `Some(backoff)` if a restart
is allowed within the current sliding window, otherwise `None`. A
denial escalates to the parent supervisor per A42.

Backoff jitter uses a tiny deterministic LCG seeded from `RestartTracker::new`'s
`rng_seed` field (default `0xDEAD_BEEF`). The picked backoff is
uniform on `[backoff_min, backoff_max)`. Deterministic mode reuses
the same seeded RNG path so two runs with the same seed pick the
same backoffs.

## Child failures

```rust
pub enum ChildFailure {
    Panic(String),
    Budget(String),
    Deadline,
}
```

The `From<RuntimeError>` impl maps runtime errors to child failures:

- `RuntimeError::BudgetExceeded(k)` → `ChildFailure::Budget(k)`
- `RuntimeError::DeadlineExceeded(_)` → `ChildFailure::Deadline`
- Anything else → `ChildFailure::Panic(error.to_string())`

## Slice-7 scope vs spec

Slice 7 ships the primitives and the policy machinery; the wiring
between an agent crash and supervisor restart is **partial**:

- The agent loop catches per-turn errors and removes the agent from
  the registry on failure.
- Supervisor restart of the child task is **not** automatically
  triggered in slice 7's per-agent loop; programmatic restart works
  by spawning a fresh agent. Full automatic restart wires in slice 8.

This matches the slice-7 exit criterion of §31.5: "supervisors restart
failed agents" — the *mechanism* is in place (RestartTracker decides
yes/no; backoff is sampled; ChildFailure is emitted) but the orchestrator
that calls `spawn_agent_loop` again is a slice-8 deliverable.

## Tests

`crates/sdust-runtime/tests/supervisor_strategies.rs`:

1. Strategy enum ordering matches the int discriminants.
2. `RestartTracker` denies the (N+1)-th attempt within the window.
3. Backoff samples fall within the configured range.
4. `ChildFailure::from(RuntimeError)` maps correctly per variant.

## See also

- `docs/internals/runtime.md`
- `docs/spec/v0.1-amendments.md` — A42
