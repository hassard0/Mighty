# Sandbox enforcement (v0.5)

> Status — implemented for v0.5 dogfood. Gaps 4 and 5 in
> [`DEMOS_V0_4_NOTES.md`](https://github.com/hassard0/Mighty/blob/main/DEMOS_V0_4_NOTES.md) are closed by
> this document.

Mighty ships two complementary enforcement mechanisms for the
[`sandbox`](../spec/v0.1.md#161-sandboxes) block:

1. **CPU / memory budgets** — caught inside the MtyIR interpreter as
   typed [`RunResult`](https://github.com/hassard0/Mighty/blob/main/crates/mty-sir/src/interp/run.rs)
   variants.
2. **Capability allow-lists** — enforced at the host bridge between
   the MtyIR interpreter and `mty-stdlib`, via a process-wide
   default cap installed by the driver.

The two share the same source of truth (`BudgetTracker` on the
runtime side, parsed from the agent's `sandbox` manifest) but route
through different code paths because they trigger at different
times.

## Budget charging — CPU steps + memory

`mty-sir::interp::run::Interp` tracks two scalars:

- `budget: u64` — remaining "steps". Every executed statement +
  terminator decrements it. When it hits 0 the interpreter returns
  [`RunResult::BudgetExceeded`].
- `mem_used: u64` / `mem_budget: u64` — total bytes "charged" so
  far + the per-run ceiling. Set via
  `run_fn_with_resource_budget(..., step_budget, mem_budget)`.
  `mem_budget == 0` is the legacy "no cap" sentinel — pre-v0.5
  callers (`run_fn_with_budget`) hit this path implicitly.

The interpreter charges memory at every "allocation-shape" rvalue
evaluation:

- `Rvalue::AdtInit` (struct / enum) — 24 B header + sum of field
  byte estimates
- `Rvalue::TupleInit` — 16 B header + sum of element estimates
- `Rvalue::ArrayInit` — 24 B header + sum of element estimates

`estimate_value_bytes` is intentionally approximate (matches the
shape of `size_of::<Value>` on 64-bit hosts plus inline string
payloads). The contract is "deterministic + monotonic", not
"bit-perfect".

When `mem_used > mem_budget` after a charge:

1. `charge_mem` returns an `Err(("MT5009", message))` which the
   `eval_rvalue` path lifts into [`EvalOutcome::Trap`].
2. The interpreter's main loop catches the next step boundary,
   notices `mem_used > mem_budget`, and returns
   [`RunResult::MemBudgetExceeded { used, limit }`].
3. Callers map the variant onto exit code `4` and an MT5009 trap
   message. See `crates/mty-driver/src/pipeline.rs::run_file_with_runtime`.

## Capability allow-lists — fs.read / fs.write

`mty-stdlib::fs::FsCap` is the in-process representation of a
capability value. It carries an `allowed: Vec<PathBuf>` list; an
empty list is "unrestricted" (tests + un-sandboxed CLI runs).

`FsCap::check(path)` succeeds if any allowed prefix matches the
incoming path; otherwise it returns
[`IoErr::Forbidden(path.display())`].

### Process-wide default cap

The stdlib's `host::dispatch` is invoked from the MtyIR interpreter's
`EffectOp::GenericCall` path without an explicit cap value (today's
MtyIR lowerer doesn't materialise per-call caps from the sandbox
manifest yet — that's the v0.6 work). To still enforce the
manifest's `fs.read = [...]` allow-list, the dispatcher consults a
process-wide default cap:

```rust
use sdust_stdlib::fs::{install_default_read_cap, FsCap};

// Driver setup — called once per sandboxed run:
install_default_read_cap(FsCap::rooted(["./inputs", "./models"]));
install_default_write_cap(FsCap::rooted(["./outputs"]));
```

Subsequent `std.fs.read("./outside")` calls land in
`host::fs_read`, which clones the current cap via
`current_default_read_cap()` and short-circuits to
`IoErr::Forbidden(...)`. The Mighty-visible result is a
[`Result::Err(forbidden:<path>)`] variant — agents can pattern-match
on it just like any other failure.

### Why the global slot?

The MtyIR dispatcher signature is `(path, method, args) -> Value`. It
has no `&mut Interp` and no way to thread per-agent state through. A
global slot lets the driver install the cap once at agent startup
without touching the MtyIR shape.

When the v0.6 lowerer materialises per-call caps, this global slot
becomes the *fallback* path: if the call shape carries an explicit
cap, the host uses that; otherwise it falls through to the default
cap.

## Tests

- `crates/mty-sir/tests/budget_charges.rs` — CPU + mem trips.
- `crates/mty-stdlib/tests/fs_capability_allowlist.rs` —
  Forbidden surface + install/restore roundtrip.

## v0.6 follow-ups

- Lower `sandbox` manifest's `fs.read = [...]` into a MtyIR
  `Rvalue::CapValue { family: Fs, constraint: PathList(...) }` and
  thread it through every `std.fs.*` call.
- Replace the heuristic `estimate_value_bytes` with a proper arena
  allocator that the runtime sizes per agent.
- Extend the cap surface to `net` (host allowlist already exists in
  `BudgetTracker::check_host`; needs the same `install_default_*_cap`
  wiring inside `host::http_get` / `http_post`).
