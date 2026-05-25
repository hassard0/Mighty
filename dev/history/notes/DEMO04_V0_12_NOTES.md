# Demo 04 `kvstore` — v0.12 implementation notes

Decisions, interpretation calls, and the language gaps surfaced
while building the 4th showcase demo
([`demos/04_kvstore/`](../../../demos/04_kvstore/)). Each entry
below is a deliberate v0.11→v0.12 stopgap; none are silent
compromises.

The demo's *story* — sharded, supervised, in-memory KV store
serving PUT/GET/DELETE through an HTTP-shaped frontend, with a
"crash a shard and watch the system survive" scene — runs
end-to-end under `mty run` today. The pieces that lean on
not-yet-wired runtime drops are itemised below.

## What the demo demonstrates (per the brief)

| Slice-7 / v0.11 feature                              | Touched? | How                                  |
|------------------------------------------------------|---------:|--------------------------------------|
| `agent` + `protocol` declarations                    |        ✓ | 5 agents, 4 protocols                 |
| `?Msg(args)` ask                                     |        ✓ | every routing operation               |
| Per-agent isolated state                             |        ✓ | each `Shard` owns its `keys[]/vals[]` |
| `supervisor S(strategy: one_for_one)` block          |        ✓ | declared at top level                 |
| `on_fail(child) { restart up_to N in DUR }`          |        ✓ | per-shard policy                      |
| `on_fail(child) { backoff D1..D2; restart }`         |        ✓ | coord + frontend policy               |
| `panic(...)` in handler body                         |        ✓ | the `Crash` message                   |
| `if / else if / else` chain control flow             |        ✓ | router fan-out                        |
| `while` + `let mut`                                  |        ✓ | array scans, hash loop                |
| String methods (`bytes`, `starts_with`, `replace`)   |        ✓ | hash + path parsing                   |
| Array methods (`push`, `get`, `len`)                 |        ✓ | shard state                           |
| `log(...)` for trace                                 |        ✓ | every milestone                       |
| `package` + `use` import                             |        ✓ | `use std.trace`                       |

## Decision log

### D1 — Drive the `Frontend` agent from `main()` (not via `http.serve`)

**Why.** `std.http.serve` ships as a real socket binder via
`crates/mty-stdlib/src/http_server.rs::start_blocking` (v0.5 dogfood
Gap-1). The runtime hook that lets it dispatch each incoming request
to the owning agent's mailbox is still TBD — see
`install_agent_dispatch` and the default-200 placeholder. Until
that lands, the SIR interpreter can't be on the receiving end of an
HTTP request.

**Stopgap.** The `Frontend` agent's `Request(method, path, body)`
handler exists in source and parses + lowers cleanly; main() drives
the same shape through a small `http_demo()` helper that feeds the
same `route_*` calls. The smoke script asserts the same
`{"PUT":...,"GET":...,"DELETE":...}` envelope that the live
frontend would emit.

**Follow-up.** Wire `install_agent_dispatch` into the SIR
interpreter so `std.http.serve(":NNNN", frontend)` actually pumps
into `Frontend.on Request(method, path, body)`. With that drop the
demo's `http_demo` helper becomes a one-line `mty run` plus a few
`curl` calls in `smoke.sh`.

### D2 — Use `SetShardId(I32)` init message instead of constructor args

**Why.** `agent Shard(id: I32) { ... }` parses but typed
constructor parameters trip `expected R_PAREN, got ':'` in v0.11.
Untyped form `agent Shard(id)` parses but the `id` argument is not
threaded into the agent's state slot (the agent boots with empty
state).

**Stopgap.** Each `Shard` ships a `SetShardId(id: I32) -> I32`
message; main() calls it on every freshly-spawned shard before
serving traffic. Adds 3 lines of boilerplate to main() per
shard-instance.

**Follow-up.** v0.12 agent-constructor-thread-through (see RFC-006
note re live migration; the same path would land typed ctor params).

### D3 — Composite agents (`Coordinator`, `Frontend`) hold `()` placeholders

**Why.** A `Coordinator` agent that needs to ask three `Shard`
agents requires `AgentRef[Shard]` slots in its state. v0.11's
agent-state schema accepts the slot but there is no language
syntax yet for "store these spawned handles into the parent
agent's state". The expected shape is a constructor `Coordinator(s0, s1, s2)`
plus typed state fields, blocked on D2's gap.

**Stopgap.** Each composite agent declares its slots as `()`
(Unit) placeholders so the protocol shape compiles + the agent
body type-checks. The actual routing happens through standalone
`route_put / route_get / route_del / route_crash` helpers that
hold the handles in `let` bindings. Mirrors what the agent body
would do.

**Cost.** Code duplication between the `Coordinator` agent body
and the `route_*` helpers (≈40 LOC). Kept both for clarity: the
agent body documents the *intended* shape, the helpers do the
*actual* work.

**Follow-up.** Once D2 lands, fold both into a single Coordinator
implementation.

### D4 — Roll our own hash (`shard_of`) instead of `std.hash`

**Why.** `std.hash` is not in the stdlib yet. `crates/mty-stdlib`
ships json, fs, http, time, tls, test — no hash module.

**Stopgap.** A ~12-line byte-rolling DJB2 (`acc * 33 + b`) in
Mighty source. Pure function over `Str`, fully deterministic, no
external dependency. Distributes the 7-key test set across the 3
shards as `[2, 2, 3]` — close enough to uniform for demo purposes.

**Follow-up.** v0.12 `std.hash` with FNV-1a / xxHash3 surfaces.

### D5 — Parallel arrays (`keys[] / vals[]`) instead of `Map[Str, Str]`

**Why.** `crates/mty-ir/src/interp/value.rs` has a Map placeholder
("stored as Array of 2-tuples in slice 6") but the actual `Map::[K,V]{}`
literal type-checks but doesn't have working methods bound to the
interpreter's eval_method table. Examples 19 (`backend_service.mty`)
uses `Map::[Str, Json]{}` in source but it's not exercised in any
running demo.

**Stopgap.** Each `Shard` owns two parallel `[]` arrays: `keys`
and `vals`. PUT linearly scans for overwrite-vs-insert; DELETE
writes a `__TOMB__` sentinel into the value slot (iteration order
stays stable across "restarts"). GET does an O(n) lookup.

**Cost.** O(n) per operation instead of O(1). For the demo's
7-key workload, sub-microsecond either way; the shape is real
KV-store semantics + the interface is identical.

**Follow-up.** Once Map literals are wired through the interp,
the `Shard` body collapses to ~10 lines:

```mty
agent Shard: Store {
  data = Map::[Str, Str]{}
  on Put(k, v) { data[k] = v; 1 }
  on Get(k)    { data.get(k).unwrap_or("__MISS__") }
  ...
}
```

### D6 — Branch-arm move-after-return trips MT3001

**Symptom.** The first cut of `Frontend.on Request(method, path, body)`
used the early-return pattern:

```mty
if path == "/stats" {
  return coord?Stats()
}
if path.starts_with("/kv/") {
  let k = path.replace("/kv/", "")
  if method == "PUT" { return coord?Put(k, body) }
  if method == "GET" { return coord?Get(k) }
  if method == "DELETE" { return coord?Del(k) }
}
"{\"error\":\"bad request\"}"
```

The borrow checker flagged `k` and `path` as moved across the
three early-return branches even though each branch is mutually
exclusive. (Same shape works fine when the return value is bound
to a `let mut` and emitted at the end of the block — see the
final source.)

**Stopgap.** Use a `let mut out = "{...default...}"` + `if/else if`
chain that single-assigns `out`. ~5 line increase.

**Follow-up.** v0.12 borrow-checker should treat each
early-return branch as a terminating point that releases the
move on the other arms.

### D7 — Supervisor block is declarative; main() does the spawning

**Symptom.** The top-level `supervisor KvTree(strategy: one_for_one) { child s0 = spawn Shard() ... }`
block parses + records the RestartPolicy + child topology in the
runtime metadata. But the runtime doesn't yet pluck the child
handles back out and pass them to main() (no "supervised tree
returns the handles to userland" plumbing).

**Stopgap.** main() does its own `spawn Counter() / spawn Shard() / ...`
calls; the supervisor block is purely declarative for now (per
spec §15 / `docs/internals/supervisors.md` slice-7 partial-restart
note). When v0.12's automatic-restart wiring lands the block's
declarations *will* be live; until then the same `panic(...)` shape
is trapped by the per-agent loop (which is why the demo's
post-crash GETs still hit).

**Follow-up.** Two-way: either main() reads child handles back
from the supervisor block (à la `SearchFlow.planner`), or the
supervisor block's child decls are top-level bindings visible to
following fn bodies. Pick one per RFC-007 (TBD).

### D8 — `panic()` inside agent handler is trapped + agent stays alive

**Symptom (intended behaviour for v0.11).** Demo's `route_crash(...)`
sends a `Crash` ask to one of the shards. The shard's handler
calls `panic("shard N crashed on purpose")`. Slice-7's per-agent
loop catches the panic (prints the message), drops the in-flight
turn's reply (`s0?Crash()` returns `()` instead of an `I32`), and
keeps the mailbox alive. Subsequent GETs to the same shard see
the *same* in-memory state — there's no restart-replay yet.

This is the documented slice-7 trip per
`docs/internals/supervisors.md`: "Slice 7 ships the primitives
and the policy machinery; the wiring between an agent crash and
supervisor restart is **partial**."

**Future.** v0.12 supervisor wiring will replace the
"keep-alive" trap with a fresh `spawn Shard()` + replay. The
demo's smoke script's `post_crash_alpha` / `post_crash_delta`
assertions will need to flip from "hit" to "miss" — or, with
state-replay, stay "hit" but via a different code path.

## Open gaps surfaced for v0.12 backlog

- [ ] Wire `std.http.serve(addr, agent)` through the SIR
      interpreter (Demo 01 + Demo 04 both block on this)
- [ ] Typed agent constructor parameters propagate to state slots
- [ ] Composite agent state slots accept `AgentRef[T]` literals
- [ ] `Map::[K, V]{}` literal methods bound in `eval_method`
- [ ] `std.hash` (FNV-1a + xxHash3)
- [ ] Branch-arm move analysis treats early-return as a release
      point (MT3001 false positive on mutually-exclusive arms)
- [ ] Supervisor block's child decls visible to surrounding scope
      (so main() doesn't have to re-spawn)
- [ ] Supervisor auto-restart wires `ChildFailure::Panic` to a
      fresh `spawn` + state-replay path

## Source-level stats

| Metric                                       | Value      |
|----------------------------------------------|-----------:|
| `.mty` source LOC (incl. comments)           | 614        |
| `.mty` source LOC (code only)                | ≈400       |
| Agents declared                              | 5          |
| Protocols declared                           | 4          |
| Top-level fns                                | 7          |
| Smoke checks (bash + PS1)                    | 22 each    |
| End-to-end `mty run` wall time (debug build) | <1 second  |

## Cross-references

- [`demos/04_kvstore/README.md`](../../../demos/04_kvstore/README.md)
- [`docs/demos/index.md`](../../../docs/demos/index.md)
- [`docs/internals/supervisors.md`](../../../docs/internals/supervisors.md)
- [`docs/spec/v0.1-amendments.md`](../../../docs/spec/v0.1-amendments.md) — A36 (`std.http.serve`), A37 (memory budget), A42 (restart policy)
