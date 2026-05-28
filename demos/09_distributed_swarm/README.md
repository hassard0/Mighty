# Demo 09 — Distributed Swarm Code Review

A 2-node Mighty program that fans a code-review request across two
cluster nodes: node-A's reviewer runs a 3-provider local swarm,
then asks a node-B sibling for its independent verdict, and joins
the two into a single rendered consensus. The forcing-function
demo for Mighty's **v0.29 cluster + typed-return + native-replay**
stack.

## What this demonstrates

| Surface | What this demo does |
|---|---|
| `BuiltinId::Swarm` interpreter arm | `swarm(...)` fires the typed builtin path under `mty run` (v0.29 Track A). Both files call `swarm(prompt, panel, budget, ConsensusStrategy.Majority)` and read `consensus.majority` / `consensus.total_cost_cents`. |
| Handler-safe swarm ADTs | `Member` / `DollarBudget` / `ConsensusStrategy` / `Consensus` constructible inside strict handler scope (v0.29 Track B). Both agents build their panels + budgets **inside** `on Review(...)`. |
| Typed bang-send returns | `let r: Str = agent ! Msg(x)` types `r` as `Str` at the call site (v0.29 Track C). `let sibling_verdict: Str = sibling ! Review(snippet)`. |
| `while let` parser | Source-level streaming surface — production demos use `while let Some(d) = stream.next() { ... }` to drain `MessageStream` (v0.29 Track D). |
| `budget` soft keyword | Natural identifier `budget` available in binding position (v0.29 Track E) — `let budget = DollarBudget.from_dollars(0.50)`. |
| `std.eval` native replay | `Replay::with_provider` + `iter_llm_calls` + trace wire v3 (v0.29 Track F). Architectural pattern for replaying the cluster hop deterministically. |
| Cluster mesh | Cross-node `?Review(snippet)` over framed CBOR + mTLS; routing decided from the manifest. |

Brought to its current shape by **v0.29 (Tracks A–F)**. All six
v0.30 follow-ups remained open at v0.29 boundary; they're tracked
inline below.

## Run

### Default — single-node short-circuit

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

When no `MTY_NODE_ID` is set the runtime treats `spawn Sibling()`
as a normal in-process spawn — the demo type-checks + runs
end-to-end without a second process, so CI is straightforward.

### Two-process cluster

Under `MTY_NODE_ID=node-b` the runtime's cluster-mesh router
(`docs/internals/cluster.md`) intercepts the bang-send and ships
the `Review(snippet)` frame as length-prefixed CBOR over TLS to the
node-B peer; the reply travels back over the same channel. The
source shape is **identical** between single- and multi-node
deployments — only the manifest and `MTY_NODE_ID` differ.

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

The `[cluster]` and `[[cluster.peers]]` blocks in `mighty.toml`
wire the two nodes via 127.0.0.1:18181 (node-a listen) and
127.0.0.1:18182 (node-b listen). The mesh runs framed CBOR over
TLS; see [`docs/internals/cluster.md`](../../docs/internals/cluster.md)
for the full wire protocol.

## Smoke

```bash
# Default — check / fmt + surface markers, no cluster spawn.
$ bash demos/09_distributed_swarm/smoke.sh

# Opt-in — two-process cluster spawn + hop verification.
$ MTY_CLUSTER_SMOKE=1 bash demos/09_distributed_swarm/smoke.sh
```

## Files

```
demos/09_distributed_swarm/
├── README.md                              ← this file
├── mighty.toml                            ← `[cluster]` + `[[cluster.peers]]`
├── smoke.sh
└── src/
    ├── main.mty                           ← node-a reviewer (joins local + sibling)
    └── sibling.mty                        ← node-b sibling (local swarm only)
```

## v0.30 follow-ups (open)

- **Explicit `AgentAddr` source surface.** The cross-node `spawn`
  shape is implicit today — the runtime decides in-process vs
  cluster-mesh routing from the manifest. A more explicit
  `AgentAddr` (peer node + agent type + agent id) is on the
  v0.31 roadmap so demos can shape-test routing failures.
- **Replay-drive the sibling short-circuit.** The default
  single-node mode just spawns `Sibling` in-process; a v0.31
  follow-up will replay-drive the sibling's swarm call through
  `std.eval` so the demo's offline mode exercises the same
  deterministic seam as the recorded production traces.
- **Structured `tool_uses` in `Member::ask`.** `swarm(...)`
  consumers can't currently see which tools the panel members
  invoked. Tracked in
  `dev/history/notes/EVAL_NATIVE_V0_29_NOTES.md` §"v0.30
  candidates"; folded into the v0.31 backlog.
