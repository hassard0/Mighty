# RFC-005 — Agent affinity front-end syntax

**Status:** Draft (v0.9 spec-freeze prep).
**Tracks amendment:** A102 (affinity hints — runtime API frozen, front-
end syntax `agent X with affinity = sticky` reserved-but-not-parsed
in v1.0).
**Target release:** v1.1.
**Owner:** *unassigned* — design owner needed before promotion.

## Implementation Status

**NOT YET SHIPPED.** Forward-looking RFC. The A102 contract —
runtime `Affinity::Sticky` / `Affinity::Elastic` API only — remains
the v1.0 surface. Front-end syntax `agent X with affinity = sticky`
is reserved-but-not-parsed.

Adjacent v0.13..v0.23 work:

* **v0.22** — Per-message work-stealing (Tier 5) replaced the v0.10
  affinity-hint scheduler with true crossbeam-deque per-worker queues
  + NUMA-locality steal ordering. The `Affinity` runtime API is
  unchanged; the underlying scheduler is now stronger.
* **v0.20** — Cluster `PlacementPolicy` trait + 3 bundled policies
  (RFC-006). These are **cluster-scope** placement and complementary
  to RFC-005 (which is **worker-scope** within a node), so RFC-005
  remains substantive.

The v1.1 surface is unchanged:

```mty
agent X(ctor_args, ...): MyAgent with affinity = sticky
agent Y(ctor_args, ...): MyAgent with affinity = elastic
agent Z(ctor_args, ...): MyAgent                          // implicit elastic
```

The window opened 2026-05-26 (close 2026-06-09, 14-day track —
shortest) is **substantive** and **closes first**.

Cross-references:

* [`v1.0-rc.md`](../v1.0-rc.md) §25.2 — agent affinity (A102).
* [`RFC_DASHBOARD.md`](RFC_DASHBOARD.md) — live window status.

## Summary

Promote the v0.6 `Affinity` runtime API (`Sticky` / `Elastic`) to a
**first-class declaration in agent spawn syntax**. Allow the developer
to opt in or out of scheduler migration at the spawn site without
plumbing the affinity through several layers of helper code.

Grammar:

```mty
agent X(ctor_args, ...): MyAgent with affinity = sticky
agent Y(ctor_args, ...): MyAgent with affinity = elastic   // explicit default
agent Z(ctor_args, ...): MyAgent                            // implicit elastic
```

Optional additional `with` clauses (already reserved by the v0.5
sandbox manifest grammar) compose:

```mty
agent W(...): MyAgent with affinity = sticky, worker = 2
```

## Motivation

The v0.6 multi-worker scheduler (A101) ships a `RuntimeBuilder::
spawn_agent_with_affinity` API that lets a host pin agents to worker 0
or hand them to the elastic load monitor. But this is a **host-side
API**: a developer writing Mighty source has no way to express the
affinity at the declaration site. Three consequences:

1. **Helper-fn proliferation.** Real apps wrap every agent spawn in a
   `fn make_metrics_agent(...) { runtime.spawn_with_affinity(Sticky,
   ...) }` shim. Source-level intent is hidden behind the helper.
2. **No static analysis.** A linter can't warn "agent declared as
   `with affinity = sticky` but spawned from inside an elastic supervisor
   tree" because the affinity isn't visible in source.
3. **Inconsistent with sandbox / budget syntax.** Sandboxes already
   use the `with { entries }` form. Affinity is a similar opt-in but
   v1.0 forces it through a different surface.

## Detailed design

### Surface syntax

```ebnf
AGENT_DECL      = "agent" IDENT TYPE_ARGS? "(" ARGS ")" (":" RETURN_TY)?
                  ("with" AFFINITY_CLAUSE)? BLOCK ;

AFFINITY_CLAUSE = AFFINITY_ENTRY ("," AFFINITY_ENTRY)* ;
AFFINITY_ENTRY  = "affinity" "=" AFFINITY_VAL
                | "worker"   "=" INT_LITERAL
                | "priority" "=" PRIORITY_VAL ;

AFFINITY_VAL    = "sticky" | "elastic" ;
PRIORITY_VAL    = "high" | "normal" | "low" ;       // reserved for RFC-005.b
```

The `with` clause is **optional**. Omitting it is equivalent to
`with affinity = elastic` (the v0.6 default).

Multiple `with` entries are comma-separated:

```mty
agent FastWriter(out): Writer with affinity = sticky, worker = 0
```

### Spawn lowering

`agent` declarations are syntactic sugar; HIR lowering already
converts them to `Spawn` rvalues. v1.1 extends `HirRvalue::Spawn`:

```rust
HirRvalue::Spawn {
    agent_ty: AdtId,
    ctor_args: Vec<ExprId>,
    affinity: Option<Affinity>,    // RFC-005: from `with affinity =`
    pinned_worker: Option<u16>,    // RFC-005: from `with worker =`
}
```

The MtyIR layer threads these through to `Stmt::Spawn { ..., affinity,
pinned_worker }`, which the runtime ABI bridge passes into
`Scheduler::spawn_with_affinity` (already wired in v0.6).

### Default behaviour

- No `with` clause → `affinity = elastic`, `pinned_worker = None`.
- `with affinity = sticky` without explicit worker → pinned to worker
  0 at spawn (existing v0.6 default for `Affinity::Sticky`).
- `with affinity = sticky, worker = N` → pinned to worker N. The
  type checker accepts any `N : u16`; runtime traps with
  `MT5021 invalid_worker_pin` if `N >= scheduler.worker_count()`.
- `with affinity = elastic, worker = N` → `MT2032 affinity_pin_conflict`
  at type-check time (a pin contradicts elasticity).

### Validation rules

| Rule                                                       | Diagnostic                |
|------------------------------------------------------------|---------------------------|
| `worker = N` with `N >= worker_count` at spawn             | MT5021 (runtime)          |
| `affinity = elastic` + `worker = N`                        | MT2032 (typeck)           |
| Duplicate `affinity =` or `worker =` entries in same `with`| MT2033 duplicate_with_entry |
| Unknown affinity value (e.g. `affinity = pinned`)          | MT2034 unknown_affinity_value |

### Supervisor inheritance

If an agent declared with `affinity = sticky` is restarted by a
supervisor (A42 restart window), the restarted instance inherits the
same affinity unless the supervisor explicitly overrides:

```mty
supervisor App {
  agent metrics(): Metrics with affinity = sticky   // inherits on restart
  agent worker(): Worker                             // elastic
}
```

The supervisor declaration syntax already supports `with` clauses for
restart policies; affinity slots in alongside them.

### Editor / LSP integration

- Semantic-token classifier (A74) gains an `affinity` keyword token
  type so editors highlight it correctly.
- Hover on an agent declaration shows the resolved affinity (the
  default-elastic case explicitly noted).
- `mty doc` HTML generator surfaces the affinity in the agent's
  card.

## Drawbacks

- **Grammar growth.** Adds two new keywords-in-context (`affinity`,
  `worker`) inside a `with` clause. They are *not* reserved
  identifiers globally — keyword-tolerant `.method` / `.field` rule
  (A3) ensures `obj.affinity` still parses as a field access. But the
  spec's keyword set grows.
- **Default-elastic ambiguity.** Some readers may expect `with` to
  always be explicit; defaulting to elastic is a convenience but a
  surprise. Mitigated by `mty doc` always showing the resolved value.
- **Forward compatibility.** A future affinity model that's neither
  sticky nor elastic (e.g. "cohort": pin to a worker subset) would
  need a fresh keyword. Reservations: `cohort = NAME` and
  `affinity = pinned` are flagged in unresolved questions below.

## Alternatives considered

1. **Attribute syntax** (`#[affinity(sticky)] agent X(...)`). More
   uniform with `#[derive(...)]` but heavier-weight; rejected as
   over-decorated for what's really a spawn-site config.
2. **Keep host-API only.** Defeats motivation.
3. **Implicit affinity from agent type** (a marker trait
   `StickyAgent`). Forces the policy into the type system but
   prevents an app from spawning the same agent type with different
   policies in different contexts; rejected.
4. **Numeric affinity weights** (`affinity = 0.7`). More expressive
   but the v0.6 scheduler doesn't honour weights; would need full
   load-balancer redesign. Reconsider in RFC-005.b.

## Unresolved questions

- Should `worker = N` permit a runtime expression (e.g.
  `worker = job_id % WORKER_COUNT`)? v1.1 starts with literal-only
  for static analysis; relax in v1.2 if real usage demands.
- Cohort affinity: should the v1.1 grammar already reserve
  `cohort = NAME` so we don't break syntax later? Recommendation:
  yes, reserve the keyword now, leave it as `MT2035 unsupported_in_v1_1`
  until v1.2 ships the cohort scheduler.
- Interaction with deterministic mode (A39): deterministic mode pins
  worker count to 1. `with worker = 2` under deterministic mode is
  `MT2036 affinity_conflicts_deterministic`. Or do we silently coerce
  to worker 0? Initial answer: hard error.

## Adoption plan

1. **v1.1-alpha.1:** parser accepts the `with` clause; HIR threads
   affinity + pinned_worker; runtime continues to use the v0.6 API.
   `MT2032`..`MT2036` diagnostics ship.
2. **v1.1-alpha.2:** LSP semantic-token + hover support.
3. **v1.1-beta:** supervisor inheritance lands; conformance corpus
   adds an `affinity/` category.
4. **v1.1.0:** A102 reclassified FROZEN; `RuntimeBuilder::spawn_agent`
   becomes a thin wrapper over the new declaration form.

A 14-day public comment window opens with v1.1-alpha.1 (shorter than
RFC-001/002 because the surface area is narrower).
