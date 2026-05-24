# 06 — Agents and protocols

> **Slice 7 (`v0.7.0-runtime`):** agents now actually run on a
> concurrent tokio executor with per-agent mailboxes. `spawn AgentName()`
> creates a long-lived actor; `agent!Msg(args)` enqueues fire-and-forget;
> `agent?Msg(args) @duration` enqueues a deadline-bounded ask. See
> `docs/internals/runtime.md` for the executor model.

An agent is an isolated unit of state, concurrency, failure, and
capability. Agents communicate by typed messages described by a
**protocol**.

## A trivial agent

```sd
protocol Echo {
  Ping(msg: Str) -> Str
}

agent Echoer: Echo {
  on Ping(msg) -> msg
}
```

- `protocol Echo` declares a typed message contract: one message,
  `Ping`, taking a `Str` and replying with a `Str`.
- `agent Echoer: Echo` declares an agent that implements `Echo`. The
  `:` is the canonical short form for "implements".
- `on Ping(msg) -> msg` is the compact form for an expression-body
  handler. The parameter types and reply type are taken from the
  protocol.

Run it:

```bash
mty check examples/07_agent_echo.sd
```

## Stateful agents

```sd
protocol Count {
  Inc() -> I64
}

agent Counter: Count {
  n = 0
  on Inc() -> { n += 1; n }
}
```

- `n = 0` is agent state. The type is inferred from the initializer
  (`I64` here). State is private to the agent — there is no way to read
  or write it from outside.
- The handler body `{ n += 1; n }` is a block with two statements; the
  last expression (`n`) is its value and so is the reply to `Inc`.
- Each invocation of `Inc` runs to completion before the next message is
  processed; state mutations are race-free by construction.

Run it:

```bash
mty check examples/08_agent_state.sd
```

## Isolation rules

From [spec §12.5](../spec/v0.1.md):

- Agent state is isolated.
- Messages crossing agents must be owned, immutable shared, copyable, or
  serialized.
- Managed heap references cannot cross agents.
- Capabilities must be explicitly passed.
- Agent failure does not corrupt other agents.

Slice 1 parses these constructs and lowers them to HIR. Enforcing the
rules happens in later slices.

## Next

Continue to [07 — Send, ask, deadlines](07-send-ask.md).
