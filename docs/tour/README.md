# A Tour of Stardust

The tour walks through the twenty canonical example programs that ship
with the compiler. Each chapter introduces one or two language features,
shows the source, explains what is interesting about it, and tells you how
to feed it to `sdust check`.

If you have not installed the compiler yet, see
[../getting-started.md](../getting-started.md).

## Contents

1. [Hello, Stardust](01-hello.md) — the smallest program.
2. [Types](02-types.md) — structs, enums, type aliases, pattern matching.
3. [Generics](03-generics.md) — generic functions and `Option`.
4. [Errors](04-errors.md) — the `T!E` sugar and `?` propagation.
5. [Control flow](05-control-flow.md) — `for`, `while`, `loop`, `match`.
6. [Agents and protocols](06-agents.md) — first-class concurrent units.
7. [Send, ask, deadlines](07-send-ask.md) — `!Msg`, `?Msg`, `@duration`.
8. [Supervisors](08-supervisors.md) — failure boundaries.
9. [Arenas](09-arenas.md) — scoped allocation.
10. [Capabilities](10-capabilities.md) — authority as parameters.
11. [Budgets](11-budgets.md) — bounded resource use.
12. [Extern](12-extern.md) — C and JavaScript interop.
13. [Unsafe](13-unsafe.md) — raw memory, contracts, audit metadata.
14. [Ownership](14-ownership.md) — moves, borrows, drop, arena escape,
    cross-agent Sendable.

After chapter 14, examples 16–20 show macros, sandboxes, and complete
backend and frontend programs. Those are not yet split into individual
tour chapters; read them directly under
[`examples/`](../../examples/).

## Running every example

```bash
for f in examples/*.sd; do sdust check "$f"; done
```

Every shipped example parses, format-roundtrips, and HIR-lowers
cleanly as of slice 2 (`v0.2.0-phase1-polish`). Slice 2 restored
examples 19 and 20 to their spec-original syntax using lambdas,
turbofish, and `if let`; see `docs/spec/v0.1-amendments.md`.

## Reading order

Start at chapter 1 and work forward. The order is the order of the
example files (`01_*.sd` through `13_*.sd`), and later chapters assume
the syntax introduced earlier.
