# 06 — Agents and protocols

> **Status:** runtime support shipped in v0.7; agents run on the
> current tokio executor with per-agent mailboxes. `spawn AgentName()`
> creates a long-lived actor; `agent!Msg(args)` enqueues
> fire-and-forget; `agent?Msg(args) @duration` enqueues a
> deadline-bounded ask. See
> [`docs/internals/runtime.md`](../internals/runtime.md) for the
> executor model.

An agent is an isolated unit of state, concurrency, failure, and
capability. Agents communicate by typed messages described by a
**protocol**.

## A trivial agent

```mty
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

## Try it

```bash
mty check examples/07_agent_echo.mty
```

## Stateful agents

```mty
protocol Count {
  Inc() -> I64
}

agent Counter: Count {
  n = 0
  on Inc() -> { n += 1; n }
}
```

- `n = 0` is agent state. The type is inferred from the initializer
  (`I64` here). State is private to the agent — there is no way to
  read or write it from outside.
- The handler body `{ n += 1; n }` is a block with two statements;
  the last expression (`n`) is its value and so is the reply to
  `Inc`.
- Each invocation of `Inc` runs to completion before the next message
  is processed; state mutations are race-free by construction.

```bash
mty check examples/08_agent_state.mty
```

## Isolation rules

From [spec §12.5](../spec/v1.0-rc.md):

- Agent state is isolated.
- Messages crossing agents must be owned, immutable shared, copyable,
  or serialized (collectively: **Sendable**).
- Managed heap references cannot cross agents.
- Capabilities must be explicitly passed.
- Agent failure does not corrupt other agents.

All five are enforced by the type / borrow / effect checkers as of
v0.10. Cross-agent Sendable violations surface as `MT3011`; capability
narrowing as `MT4010`.

## Next

Continue to [07 — Send, ask, deadlines](07-send-ask.md).
