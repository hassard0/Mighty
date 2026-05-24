# Loops: `loop`, `while`, `for` + break / continue

Status: v0.5. Replaces the slice-4 single-iteration loop semantics.

## Source syntax

```sd
loop { … }                          // infinite — exit via break / return / panic
while <cond> { … }                  // exit when cond evaluates to false
for <pat> in <iter> { … }           // exit when the iterator is exhausted
break                               // unwind to the nearest enclosing loop
break <value>                       // ... and yield <value> as the loop's value
continue                            // re-enter the loop header without running
                                    // the rest of the body
```

Labels (`'outer: loop { break 'outer }`) are deferred to v0.6 — v0.5
ships unlabelled break/continue only.

## HIR

`crates/mty-hir/src/nodes.rs`:

```rust
pub enum HirExpr {
    …
    Loop { body: BlockId },
    While { cond: ExprId, body: BlockId },
    For   { pat: PatId, iter: ExprId, body: BlockId },
    Break(Option<ExprId>),     // v0.5 — None = bare `break`, Some(e) = `break e`
    Continue,                  // v0.5
    …
}
```

`Break(None)` and `Continue` synth to `never`; `Break(Some(e))` synths
the inner expression and threads the type information into the side
table. v0.5's type checker does NOT yet unify break-values with the
enclosing loop's result type — that lands in v0.6 with labels.

## MtyIR lowering (`crates/mty-sir/src/lower/exprs.rs`)

Every loop allocates four basic blocks plus a result local:

```
                ┌────────┐
                │ header │ ← back-edge from continue_tgt
                └───┬────┘
                    │
        ┌───────────┴───────────┐
        ▼                       ▼
   ┌────────┐              ┌──────┐
   │  body  │── fall-thru ─│ exit │ ← break sets result_local then jumps here
   └───┬────┘              └──────┘
       │
       ▼
   ┌──────────────┐
   │ continue_tgt │ ── goto header
   └──────────────┘
```

* **`loop { … }`** — header is empty (just `Goto(body)`); cond is always true.
* **`while c { … }`** — header evaluates `c`; if false, terminator is `Goto(exit)`.
* **`for x in iter { … }`** — header materialises the iterator protocol probe (see
  `iterators.md`), tests the "exhausted" bool, and either binds `x` to the
  yielded element or jumps to exit.

`break` lowers to `Stmt::Assign(result_local, value)` + `Term::Goto(exit_target)`.
`continue` lowers to `Term::Goto(continue_target)`. Both then switch to
a fresh dead block so subsequent statements have somewhere to land
without being mistakenly attached to a now-terminated block.

The `FnBuilder` carries a `loop_stack: Vec<LoopFrame>` populated by
`lower_loop` / `lower_while` / `lower_for` and consulted by the
`Break` / `Continue` lowering. A stray `break` outside any loop emits
`Term::Unreachable` — the type/borrow checker has already reported
the misuse.

## Borrow checker (`crates/mty-borrow/src/flow.rs`)

Loop bodies now run through `loop_fixed_point`, which:

1. Snapshots the pre-body `locals` map + `BorrowLedger`.
2. Walks the body up to 16 times, joining the post-body state back
   into the pre-body baseline with `join_states` + `join_ledgers`
   (the same conservative joins used at `if`/`match`).
3. Stops as soon as `ledger.records.len()` is stable across two
   passes — which is when convergence is reached because the join is
   monotonic in the ledger.
4. After the cap fires, runs one final pass + join and exits.

`break` walks its inner expression at `Position::Move` (matching
`return`); `continue` is a no-op for the walker.

## Interpreter (`crates/mty-sir/src/interp/run.rs`)

No new terminator kinds — the existing `Term::Goto` + `Term::If` carry
the break/continue control flow. The step budget (`DEFAULT_STEP_BUDGET =
1_000_000`) remains the defense-in-depth cap.

The for-loop's per-iteration `__sdust_iter_next` call increases the
step count, which means bounded `for i in 0..N` always terminates
within the budget for any N below the budget.

## Self-host bootstrap

`crates/mty-driver/tests/selfhost_lexer.rs::selfhost_lexer_full_diff_against_rust`
was `#[ignore]` in v0.4 because the lexer's `loop { … if … break }`
scanning pattern parsed `break` as an identifier with no effect — the
inner loops spun until the step budget exhausted. With this slice, the
lexer terminates and the diff test is unblocked (see SELFHOST_V0_5_NOTES.md
for the actual status after the diff was re-run).
