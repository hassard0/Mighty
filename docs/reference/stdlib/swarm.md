# `std.swarm`

Multi-LLM consensus primitive. Shipped in v0.27 (Track D) — one of
the five "make Mighty the standard language for agents" features.
Built on v0.26's [`std.llm`](./llm.md) providers + `DollarBudget`.

| Surface | Shape |
|---|---|
| `swarm(prompt, panel, budget, strategy).await` | Top-level entry point |
| `Member::anthropic(model)` / `openai(model)` / `gemini(model)` / `bedrock(model)` | Panel member constructors (read provider env vars) |
| `ConsensusStrategy` | `Majority` / `Unanimous` / `WeightedVote { weights }` / `FirstAgreed` |
| `SharedDollarBudget` | One integer-cents pool shared across the panel |
| `Consensus` | Result: `majority`, `dissents`, `all_replies`, `budget_exhausted`, `strategy` |

## Mighty surface

```mty
use std.swarm

let consensus = swarm(
  prompt: "Is this code safe? <code>",
  panel: vec![
    Member.anthropic("claude-opus-4-7"),
    Member.openai("gpt-5"),
    Member.gemini("gemini-2.5-pro"),
  ],
  budget: DollarBudget.new(0.50),
  strategy: ConsensusStrategy.Majority,
).await

log(format!("verdict: {}", consensus.majority))
for dissent in consensus.dissents {
  log(format!("- {} disagrees: {}", dissent.member, dissent.body))
}
```

The `std.swarm` module is opaque to the typechecker and dispatches
through the permissive method table (same shape as `std.llm`).
Constructors like `Member.anthropic(...)`, `ConsensusStrategy.Majority`,
and methods like `.ask` are all registered as permissive.

## Rust surface

```rust
use mty_stdlib::swarm::{
    swarm, Member, ConsensusStrategy, SharedDollarBudget,
};

# async fn run() -> Result<(), mty_stdlib::swarm::SwarmError> {
let consensus = swarm(
    "Is this code safe?",
    vec![
        Member::anthropic("claude-opus-4-7"),
        Member::openai("gpt-5"),
        Member::gemini("gemini-2.5-pro"),
    ],
    SharedDollarBudget::from_dollars(0.50),
    ConsensusStrategy::Majority,
)
.await?;

println!("verdict: {:?}", consensus.majority);
for d in &consensus.dissents {
    println!("- {} disagrees: {}", d.member, d.body);
}
# Ok(()) }
```

## Consensus strategies

### `Majority`

Cluster-with-most-members wins. Dissents are the rest. Ties break by
first-cluster-to-form (deterministic across runs given a fixed input
order; parallel dispatch can still shuffle the order of replies).

```rust
ConsensusStrategy::Majority
```

### `Unanimous`

Only returns a verdict if *every* member lands in one cluster.
Otherwise `majority: None` and every reply surfaces as a dissent so
the caller sees the full split. Use this when "any disagreement is a
veto" is the call-site policy.

```rust
ConsensusStrategy::Unanimous
```

### `WeightedVote { weights }`

Each member carries a weight (positional, same order as the panel
vec); missing weights default to `1`. The cluster with the highest
*weight sum* wins.

```rust
// "Anthropic counts double; everything else single."
ConsensusStrategy::WeightedVote { weights: vec![2, 1, 1] }
```

### `FirstAgreed`

As soon as two members land in the same cluster, return immediately —
pending members are dropped. **Sequential** dispatch (the only
strategy that doesn't fan out). Use it when you care more about cost
than maximally robust consensus.

```rust
ConsensusStrategy::FirstAgreed
```

## Answer comparison

The vote tier picks a [`SimilarityMode`](https://docs.rs/mty-stdlib/latest/mty_stdlib/swarm/enum.SimilarityMode.html)
heuristically:

- **Short bodies** (all replies <= 24 chars) → `Exact`. Trim,
  lower-case, strip punctuation, exact-match. Ideal for `yes`/`no` /
  short labels.
- **Longer bodies** → `TokenSet`. Normalised-token-set Jaccard;
  threshold = `0.6`. Cheap, deterministic, no embed model required.
  "Well tested code" + "code is well-tested" cluster together.

v0.28 will add `SimilarityMode::Embedding` (cosine over `std.llm.embed`
vectors) + an optional LLM-judge mode where Claude evaluates "do
these N say the same thing".

## Shared budget

[`SharedDollarBudget`](https://docs.rs/mty-stdlib/latest/mty_stdlib/swarm/struct.SharedDollarBudget.html)
is one integer-cents pool shared across every panel member. Clone
freely — every clone shares the same `Arc<Mutex<_>>` counter.

```rust
let budget = SharedDollarBudget::from_dollars(0.50);   // 50-cent cap
budget.try_charge(20)?;                                // record 20¢
assert_eq!(budget.consumed_cents(), 20);
assert!(!budget.is_exhausted());
```

### Mid-flight exhaustion

When the pool trips while a swarm is dispatching, pending members are
dropped and the [`Consensus`] surfaces with
`budget_exhausted: true`. The members that already ran are *still*
resolved through `strategy` — you may get a `majority` even when the
budget tripped, if the members that fit reached one.

```text
panel of 4, cost=15 each, budget=35:
  member 0 → 15 consumed (under cap, dispatch)
  member 1 → 30 consumed (under cap, dispatch)
  member 2 → 45 consumed (over cap, but recorded)
  member 3 → not dispatched (is_exhausted() == true)
  Consensus.budget_exhausted = true
  Consensus.all_replies.len() == 3
```

## Errors

- [`SwarmError::EmptyPanel`] — at least one member is required.
- [`SwarmError::AllMembersFailed`] — every panel member errored;
  carries the first error string.
- [`SwarmError::Llm`] — single-member panel surfaced an LLM error
  unrecovered.

Per-member `LlmError`s on multi-member panels are *collected* into
the consensus rather than aborting the swarm: a 3-member panel where
one member 500s + two agreed still returns a valid `Consensus` with
`all_replies.len() == 2`.

## Provider status (v0.27)

`Member::anthropic(...)` consumes the full v0.26 Anthropic client
(HTTP/1.1 + streaming + budgets). The OpenAI/Gemini/Bedrock
constructors consume the v0.26 skeleton clients — their `complete()`
returns a stub `Message::assistant_text(...)` body for now. The real
wire-up for those three providers ships in v0.28; the swarm primitive
itself is provider-agnostic, so upgrading is invisible at the call
site.

For tests, [`Member::mock`] + [`Member::mock_error`] give
deterministic stand-ins with a canned reply + per-call cost. See
`crates/mty-stdlib/tests/swarm_basic.rs` / `swarm_budget.rs` /
`swarm_consensus.rs` for the full sweep.
