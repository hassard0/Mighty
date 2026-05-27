# demo 09 — distributed swarm code review

A 2-node Mighty program that fans a code-review request across two
cluster nodes: node-A's reviewer runs a 3-provider local swarm, then
asks a node-B sibling for its independent verdict, and joins the two
into a single rendered consensus.

This is the v0.29 forcing-function demo — every one of v0.29's six
shipped tracks gets exercised end-to-end across the two files:

| Track | What it ships | How this demo consumes it |
|---|---|---|
| **A** — `BuiltinId::Swarm` interpreter arm | `swarm(...)` fires the typed builtin path under `mty run` instead of `Value::Unit` | both files call `swarm(prompt, panel, budget, ConsensusStrategy.Majority)` and read `consensus.majority` / `consensus.total_cost_cents` |
| **B** — handler-safe swarm ADTs | `Member` / `DollarBudget` / `ConsensusStrategy` / `Consensus` constructible inside strict handler scope | the `Reviewer` (node-A) and `Sibling` (node-B) agents both build their panels + budgets INSIDE `on Review(...)` — zero ctor args |
| **C** — typed bang-send returns | `let r: Str = agent ! Msg(x)` types `r` as `Str` directly at the call site | `let sibling_verdict: Str = sibling ! Review(snippet)` and `let joined: Str = reviewer ! Review(snippet)` |
| **D** — `while let` parser | finished streaming surface lets Mighty source iterate deltas | architectural pattern documented inline; production demos use `while let Some(d) = stream.next() { ... }` to drain `MessageStream` |
| **E** — `budget` soft keyword | natural identifier `budget` available in every binding position | `let budget = DollarBudget.from_dollars(0.50)` in both files (pre-v0.29 forced `spend_cap`) |
| **F** — `std.eval` native replay | `Replay::with_provider` + `iter_llm_calls` + trace wire v3 | architectural pattern for replaying the cluster hop in a deterministic test is described below |

## Run

Default (single-node short-circuit — no cluster wiring; exercises the
parse/check/run pipeline including the v0.29 Track A interpreter arm
firing `swarm(...)`):

```bash
$ mty check demos/09_distributed_swarm/src/main.mty
$ mty check demos/09_distributed_swarm/src/sibling.mty

$ mty run demos/09_distributed_swarm/src/main.mty -- "fn double(x: i32) -> i32 { x * 2 }"
evt:reviewer:review
evt:reviewer:joined
distributed_swarm: joined consensus follows
node-a verdict: ...
node-a cost cents: ...
node-b verdict: (unconfigured-peer)
```

When no `MTY_NODE_ID` is set the runtime treats `spawn Sibling()` as
a normal in-process spawn — the demo type-checks + runs end-to-end
without a second process, so CI is straightforward. Under
`MTY_NODE_ID=node-b` the runtime's cluster-mesh router
(`docs/internals/cluster.md`) intercepts the bang-send and ships the
`Review(snippet)` frame as length-prefixed CBOR over TLS to the
node-B peer; the reply travels back over the same channel. The
source shape is identical between single- and multi-node
deployments — only the manifest and `MTY_NODE_ID` differ.

Live two-process cluster run:

```bash
# Terminal 1 — sibling on node-b
MTY_NODE_ID=node-b mty run demos/09_distributed_swarm/src/sibling.mty

# Terminal 2 — reviewer on node-a (with API keys for the local panel)
ANTHROPIC_API_KEY=sk-ant-... \
OPENAI_API_KEY=sk-... \
GEMINI_API_KEY=... \
MTY_NODE_ID=node-a mty run demos/09_distributed_swarm/src/main.mty \
  -- "let x = eval(user_input)"
```

The `[cluster]` and `[[cluster.peers]]` blocks in `mighty.toml` wire
the two nodes via 127.0.0.1:18181 (node-a listen) and
127.0.0.1:18182 (node-b listen). The mesh runs framed CBOR over TLS;
see `docs/internals/cluster.md` for the full wire protocol.

## Smoke

```bash
# Default — check/fmt + surface markers, no cluster spawn.
$ bash demos/09_distributed_swarm/smoke.sh

# Opt-in — two-process cluster spawn + hop verification.
$ MTY_CLUSTER_SMOKE=1 bash demos/09_distributed_swarm/smoke.sh
```

## v0.30 follow-ups

- The cross-node `spawn` shape is implicit — the runtime decides
  in-process vs cluster-mesh routing from the manifest. A more
  explicit `AgentAddr` source surface (peer node + agent type +
  agent id) is on the v0.30 roadmap so demos can shape-test routing
  failures.
- The default single-node short-circuit just spawns `Sibling`
  in-process; a v0.30 follow-up will replay-drive the sibling's
  swarm call through `std.eval` so the demo's offline mode exercises
  the same deterministic seam as the recorded production traces.
- `Member::ask` doesn't yet return structured `tool_uses` —
  `swarm(...)` consumers can't currently see which tools the panel
  members invoked. Follow-up tracked in
  `dev/history/notes/EVAL_NATIVE_V0_29_NOTES.md` §"v0.30 candidates".
