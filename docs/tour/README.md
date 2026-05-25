# A Tour of Mighty

The tour walks through the twenty canonical example programs that ship
with the compiler. Each chapter introduces one or two language
features, shows the source, explains what is interesting about it, and
tells you how to feed it to `mty check`.

If you have not installed the compiler yet, see
[../getting-started.md](../getting-started.md).

## Contents

1. [Hello, Mighty](01-hello.md) — the smallest program.
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
15. [Traits](15-traits.md) — declaration, impls, dispatch, `dyn Trait`,
    derive(Copy/Hash/Eq).

After chapter 15, examples 16–20 show macros, sandboxes, and complete
backend and frontend programs. They are not yet split into individual
tour chapters; read them directly under
[`examples/`](https://github.com/hassard0/Mighty/tree/main/examples).

## Running every example

```bash
for f in examples/*.mty; do mty check "$f"; done
```

On Windows PowerShell:

```powershell
Get-ChildItem examples/*.mty | ForEach-Object { mty check $_.FullName }
```

All twenty shipped examples parse, format-roundtrip, type-check,
borrow-check, effect-check, and run cleanly as of **v0.10**. If you
see anything other than `ok: <path>` (or the documented MT2026 warning
on `13_capabilities.mty`), open an issue.

## Reading order

Start at chapter 1 and work forward. The order follows the example
files (`01_*.mty` through `20_*.mty`), and later chapters assume the
syntax introduced earlier. Each chapter ends with a `Try it:` block
that runs the corresponding example through `mty check` and (where it
makes sense) `mty run`.

## Spec cross-references

Section numbers in this tour reference the current normative spec,
[`docs/spec/v1.0-rc.md`](../spec/v1.0-rc.md) (v1.0-RC2). The
historical v0.1 spec stub at
[`docs/spec/v0.1.md`](../spec/v0.1.md) is preserved for archaeology;
do not author against it.
