# 11 — Budgets

A `budget { ... } run expr` block bounds the resources `expr` may
consume. Budgets cover CPU time, wall time, memory, mailbox depth, and
more — see [spec §16.2](../spec/v0.1.md) for the full list.

## The program

```sd
// Note: spec uses `mb 1k`; lexer SIZE_LITERAL suffix support TBD in slice 2.
fn run_job(input: Bytes) -> Result!RunErr {
  budget {
    cpu 150ms
    wall 2s
    mem 128MiB
    mb 1024
  } run {
    job(input)?
  }
}
```

## What is interesting

- The budget body is a list of `<dimension> <value>` entries, one per
  line. The keys (`cpu`, `wall`, `mem`, `mb`) are reserved within the
  budget grammar; the values are duration or size literals.
- `run { ... }` introduces the bounded computation. The `run` keyword is
  the parser's separator between the budget header and the body.
- Budget violations surface as typed errors (`RunErr` here) or trigger
  supervisor policy — the runtime decides.
- The `// Note:` comment marks one of the slice-1 deferrals: the lexer
  does not yet recognize a `1k` suffix for raw counts, so the example
  writes `1024` literally. Slice 2 will close that gap.

For the long form (`sandbox ... with { ... } { run ... }`), see
[`examples/18_sandbox.sd`](../../examples/18_sandbox.sd) and spec §16.1.

## Run it

```bash
sdust check examples/11_budget_block.sd
```

## Next

Continue to [12 — Extern](12-extern.md).
