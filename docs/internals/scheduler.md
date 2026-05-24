# Scheduler (slice 7)

**Module:** `sdust_runtime::scheduler`
**Spec:** §25.4 Scheduling

## Default: tokio multi-thread

`Scheduler::multi_thread(threads)` builds a `tokio::runtime::Runtime`
with `worker_threads = threads.max(1)` and `enable_all()`. Slice 7
defaults `threads = 1` to match A39 (single-core MVP), overridable
with `STARDUST_RUNTIME_THREADS=N`.

Why single-thread by default in slice 7?

1. Determinism: a single worker preserves cross-agent ordering, which
   makes the slice-7 telemetry stream stable across runs.
2. Mailbox fairness is FIFO per-agent automatically.
3. Real work-stealing across cores ships in slice 8 alongside codegen.

Spec §25.4 calls for "work-stealing per core" — tokio gives us that
when we set `worker_threads > 1`. No code path needs to change; just
flip the env var. The MVP guarantee is single-core.

## Deterministic: tokio current-thread

`Scheduler::current_thread()` builds a current-thread tokio runtime.
Used when `RuntimeBuilder::deterministic(seed)` is called. Combined
with `SeededRng` and `LogicalClock` (in `deterministic.rs`) this
gives spec §25.5's "controlled/replayable" interleavings.

Slice-7 caveat (A41): cancellation triggered by a deadline only
arrives at the next await point. Inside one SIR turn the slice-6
evaluator is synchronous from the executor's view, so the deadline
cancels the *next* queued turn rather than interrupting the running
one. Turns are bounded by interpreter step budget (default 1 000 000
steps) so a single turn cannot run forever.

## Spec §25.4 alignment

| Spec property                | Slice-7 form |
|------------------------------|--------------|
| Work-stealing per core       | tokio multi-thread (configurable, default 1) |
| Cooperative cancellation     | tokio::time::timeout at await points |
| Deadline-aware polling       | `with_deadline(d, fut)` wraps reply oneshots |
| Backpressure on mailboxes    | bounded MPSC + SendPolicy::Block (default) |
| Task-local arenas            | per-turn arena push/pop in SIR interp; full alloc deferred to slice 8 |
| Agent turn fairness          | each agent is one tokio task; tokio cooperative scheduling |

## Where this fits

`Runtime::scheduler: Arc<Scheduler>` is exposed publicly; consumers
who want to run sync code can call `runtime.scheduler.rt.block_on(...)`
the same way `sdust-driver::pipeline::run_file_with_runtime` does.

## See also

- `docs/internals/runtime.md`
- `docs/spec/v0.1-amendments.md` — A39, A41
