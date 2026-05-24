# Budgets + Sandboxes (slice 7)

**Module:** `sdust_runtime::budget`
**Spec:** §16 Budgets and Sandboxing

## Budget shape

```rust
pub struct Budget {
    pub cpu: Option<Duration>,        // spec: cpu 150ms
    pub wall: Option<Duration>,       // spec: wall 2s
    pub mem_bytes: Option<u64>,       // spec: mem 128MiB / memory = ...
    pub mailbox: Option<u64>,         // spec: mb 1k / mailbox = 1024
    pub spawned: Option<u64>,         // spec: spawned tasks
    pub hosts: Option<Vec<String>>,   // spec: net = ["api.example.com:443"]
    pub read_paths: Option<Vec<String>>,  // spec: fs.read = ["/models", ...]
    pub write_paths: Option<Vec<String>>, // spec: fs.write = ["/tmp/out"]
}
```

A `None` field means **unlimited** (or no allowlist). Defaults are
all `None` so an unconfigured runtime never gates anything.

## BudgetTracker

`BudgetTracker` wraps a `Budget` with atomic counters:

- `cpu_ns: AtomicU64` — accumulated per-turn CPU duration
- `mem: AtomicU64` — synthetic byte cost (A37: 1 B per primitive,
  24 B header per allocation, etc.; real arena allocator lands in
  slice 8)
- `spawned: AtomicU64` — child agent / task count
- `start: Instant` — measured wall clock baseline

### Methods

| Method | Returns | Notes |
|--------|---------|-------|
| `record_cpu(d)` | () | adds to cpu_ns |
| `record_mem(b)` | () | adds to mem |
| `record_spawn()` | `Result<(), BudgetBreach>` | bumps + checks |
| `check()` | `Result<(), BudgetBreach>` | checks CPU/wall/mem |
| `check_mailbox_depth(d)` | `Result<(), BudgetBreach>` | check before send |
| `check_host(h)` | `Result<(), BudgetBreach>` | allowlist check |
| `check_read_path(p)` | `Result<(), BudgetBreach>` | prefix-match allowlist |
| `check_write_path(p)` | `Result<(), BudgetBreach>` | prefix-match allowlist |

### Breach → RuntimeError

```rust
BudgetBreach::Cpu(_)   → RuntimeError::BudgetExceeded("cpu ...")  → SD5009
BudgetBreach::Wall(_)  → RuntimeError::BudgetExceeded("wall ...") → SD5009
BudgetBreach::Mem(_)   → RuntimeError::BudgetExceeded("mem ...")  → SD5009
BudgetBreach::Mailbox  → RuntimeError::BudgetExceeded("mailbox")  → SD5009
BudgetBreach::Spawned  → RuntimeError::BudgetExceeded("spawned")  → SD5009
BudgetBreach::Host     → RuntimeError::CapabilityOutsideSandbox   → SD5015
BudgetBreach::Path     → RuntimeError::CapabilityOutsideSandbox   → SD5015
```

A40 retains spec §16.2's "Budget violations return typed errors or
trigger supervisor policy": the supervisor's `From<RuntimeError>` for
`ChildFailure` routes budget breaches to `ChildFailure::Budget` so
the supervisor's policy (e.g. `escalate`) applies.

## Sandbox enforcement

Slice 5 lowered `sandbox Name with { entries } { body }` to a
`HirItem::Sandbox` that the slice-6 interpreter treated as metadata.
Slice 7 ships **runtime enforcement** at the `BudgetTracker` layer:

- `fs.read = [...]` populates `read_paths`.
- `fs.write = [...]` populates `write_paths`.
- `net = [...]` populates `hosts`.
- `cpu = ...`, `wall = ...`, `memory = ...`, `mailbox = ...` populate
  the corresponding scalar fields.

Per A43, top-level sandbox items construct a child runtime with the
declared budget caps; capability calls inside the body are gated.
Nested sandboxes intersect allowlists.

## Slice-7 memory budget approximation

Per A37: without a real arena allocator (slice 8 work), `mem_bytes`
is approximate. Slice 7 records bytes on explicit `record_mem(n)`
calls only; the SIR interpreter does not auto-count allocations. This
makes the check pessimistic — programs may breach in production that
slice-7 tests miss. Slice 8 wires the real allocator.

## Tests

- `tests/budget_enforcement.rs` — CPU/wall/mem/mailbox/spawn + host/path allowlists.
- `tests/sandbox_enforcement.rs` — host + read/write path independence.

## See also

- `docs/internals/runtime.md`
- `docs/internals/supervisors.md`
- `docs/spec/v0.1-amendments.md` — A37, A43
