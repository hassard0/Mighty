# 11 — Budgets

> **Slice 7 (`v0.7.0-runtime`):** budgets and sandboxes are now
> enforced at runtime. CPU/wall/mem/mailbox/spawn counters trap with
> MT5009; host and path allowlists trap with MT5015. See
> `docs/internals/budgets.md`.

A `budget { ... } run expr` block bounds the resources `expr` may
consume. Budgets cover CPU time, wall time, memory, mailbox depth, and
more — see [spec §16.2](../spec/v0.1.md) for the full list.

## The program

```sd
fn run_job(input: Bytes) -> Result!RunErr {
  budget {
    cpu 150ms
    wall 2s
    mem 128MiB
    mb 1k
  } run {
    job(input)?
  }
}
```

## What is interesting

- The budget body is a list of `<dimension> <value>` entries, one per
  line. The keys (`cpu`, `wall`, `mem`, `mb`) are reserved within the
  budget grammar; the values are duration or size literals.
- `run { ... }` introduces the bounded computation. The `run` keyword
  separates the budget header from the body.
- `1k` is a decimal size suffix (=1000), distinct from the binary
  `1KiB` (=1024). See `docs/spec/v0.1-amendments.md` A1.
- Budget violations surface as typed errors (`RunErr` here) or trigger
  supervisor policy — the runtime decides.

For the long form (`sandbox ... with { ... } { run ... }`), see
[`examples/18_sandbox.mty`](https://github.com/hassard0/Mighty/blob/main/examples/18_sandbox.mty) and spec §16.1.

## Run it

```bash
mty check examples/11_budget_block.sd
```

## Next

Continue to [12 — Extern](12-extern.md).
