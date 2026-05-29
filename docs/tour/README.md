# A Tour of Mighty

The tour walks through the canonical example programs that ship
with the compiler. Each chapter introduces one or two language
features, shows the source, explains what is interesting about it,
and tells you how to feed it to `mty check`.

If you have not installed the compiler yet, see
[../getting-started.md](../getting-started.md).

## Contents (chapters 1–15 — language fundamentals)

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

## Contents (chapters 16–21 — agent surface, v0.26–v0.33)

16. [Tools and LLM providers](16-tools-and-llm.md) — `@tool`
    decorator, typed providers, streaming.
17. [Swarm consensus and `std.eval`](17-swarm-and-eval.md) —
    multi-provider voting + regression harness.
18. [Taint types](18-taint-types.md) — `Tainted[T]` and the three
    approved exits; prompt injection as a compile error.
19. [Observability](19-observability.md) — `std.observe` +
    `mty inspect --cost`.
20. [Computer use](20-computer-use.md) — Anthropic Computer Use as
    a capability with a typed sandbox.
21. [RAG and vision](21-rag-and-vision.md) — `std.rag.Index` +
    `std.rag.Rag` + `std.llm.Image` multi-modal pipeline (v0.33).

Examples 16–24 (macros, sandboxes, end-to-end backend / frontend
services, effect rows, agent fields) sit outside the tour
chapters because they are direct references for surfaces already
covered in earlier chapters. They live under
[`examples/`](https://github.com/hassard0/Mighty/blob/main/examples/README.md)
grouped by topic. Examples 37–40 cover post-v0.30 surfaces
(`mty dap` debug session in 37, diagnostic envelopes in 38,
v0.36 native binaries + extern C in 39, v0.36 T3 string editing
in 40).

## Running every example

```bash
for f in examples/*.mty; do mty check "$f"; done
```

On Windows PowerShell:

```powershell
Get-ChildItem examples/*.mty | ForEach-Object { mty check $_.FullName }
```

All 40 shipped examples parse, format-roundtrip, type-check,
borrow-check, effect-check, taint-check, and run cleanly as of
**v0.36**. If you see anything other than `ok: <path>` (or the
documented MT2026 warning on `13_capabilities.mty`), open an
issue.

## Reading order

For first-time readers: start at chapter 1 and work forward. The
order follows the example files (`01_*.mty` through `15_*.mty`),
and later chapters assume the syntax introduced earlier.

For experienced systems programmers who want the agent stuff: read
chapters 6–7 (agents + send/ask), 10 (capabilities), then jump to
the agent-stdlib examples 27–36 via the
[examples README](https://github.com/hassard0/Mighty/blob/main/examples/README.md).

## Spec cross-references

Section numbers in this tour reference the current normative spec,
[`docs/spec/v1.0-rc.md`](../spec/v1.0-rc.md) (v1.0-RC5). The
historical v0.1 spec stub at
[`docs/spec/v0.1.md`](../spec/v0.1.md) is preserved for
archaeology; do not author against it.
