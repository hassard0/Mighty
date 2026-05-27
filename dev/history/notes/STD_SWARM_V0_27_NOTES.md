# `std.swarm` — v0.27 Track D engineering notes

## Scope

Land a multi-LLM consensus primitive so Mighty source can ask N
panel members the same question in parallel, collect their answers,
and surface a verdict + every dissent. After this slice:

- `mty_stdlib::swarm::swarm(prompt, panel, budget, strategy).await`
  is the single entry point.
- `Member` is a tagged enum over the four `std.llm` clients
  (Anthropic / OpenAI / Gemini / Bedrock) + a `Mock` variant for
  tests.
- Four consensus strategies ship: `Majority`, `Unanimous`,
  `WeightedVote { weights }`, `FirstAgreed`.
- `SharedDollarBudget` is one integer-cents pool consulted by every
  member; mid-flight exhaustion surfaces `budget_exhausted: true`
  on the result.
- Answer comparison clusters replies via Jaccard token-set
  similarity (free-form) or exact-match (short answers).

## Design decisions

### Why a tagged enum for `Member` instead of `Box<dyn LlmProvider>`

Both shapes are technically valid — `LlmProvider` is already trait-
object friendly thanks to `async_trait` + the no-GATs trait
signature. We went with the tagged enum for three reasons:

1. **Provider-specific weight tables.** `WeightedVote` callers
   typically want different weights per provider class — a `match`
   over `Member` reads cleaner than carrying a parallel `Vec<u32>`
   alongside a `Vec<Box<dyn LlmProvider>>`.
2. **Test mocks land in the same enum.** `Member::mock(name, body,
   cost)` is the deterministic stand-in every test uses; with a
   trait-object shape we'd either need a sibling `MockProvider`
   crate or a feature-gated test type.
3. **Future variants stay typed.** v0.28 will add Ollama / local
   inference and embedding-cosine clustering; both want to drop a
   variant in `Member` rather than re-implement `LlmProvider`'s
   full streaming surface for "I just need a one-shot reply".

### Why a separate `SharedDollarBudget` rather than reusing `DollarBudget`

`std.llm`'s `DollarBudget` already supports `clone()` for shared
state, and `Arc<Mutex<_>>` semantics are identical. The split is
intentional:

- `DollarBudget` belongs to *one* completion request (the
  `CompletionRequest.dollar_budget` field). Its API surface includes
  `add_usage(model, input_tokens, output_tokens)` because the
  upstream provider hands us back a `usage` field.
- `SharedDollarBudget` belongs to *a whole panel*. Its API surface
  is integer-cents-first (`try_charge(cents)`) because the swarm
  doesn't know the input/output token split for every provider —
  the v0.26 OpenAI/Gemini/Bedrock skeletons return a stub body
  without a `usage` field, so we estimate `len()/4` tokens and use
  the per-model rate table to convert to cents.

Keeping them separate also lets the swarm log a clean
`budget_exhausted: true` flag without the typed-budget machinery
escaping into the result shape.

### Why `Majority` dispatches in parallel but `FirstAgreed` is sequential

`Majority` (and `Unanimous`, `WeightedVote`) needs *every* member's
reply before resolving — there's no early exit. Parallel dispatch
via `tokio::spawn` + a manual `Vec<JoinHandle>` collection wins on
wall-clock latency (3 members, 2s each → 2s instead of 6s).

`FirstAgreed`'s whole point is the early exit. If we dispatched in
parallel, every member would be in-flight by the time we noticed
that two had agreed — defeating the cost-saving. Sequential
dispatch + a cluster-check between each reply preserves the
"stop after the second cluster of size 2" semantics.

### Why a `len <= 24` heuristic to pick `SimilarityMode::Exact`

Real swarms ask one of two kinds of question:

- **Yes/no.** "Is this safe?" → 3-char reply. Trimming + lower-
  casing + exact-match is correct *and* cheap.
- **Free-form.** "Summarise the risks." → paragraph-length reply.
  Exact-match would miss every paraphrase ("safe to ship" vs "safe
  for production"); Jaccard over normalised token sets handles
  paraphrases without an embed model.

`24` is the cutoff because "i don't know" (12 chars), "approve"
(7), and "yes, but with caveats" (21) all fit comfortably. Longer
replies pay the Jaccard cost; in practice that's still O(n²) over
the panel which is fine for n <= 10.

### Why tokens split on `is_alphanumeric()` rather than only whitespace

First pass split only on whitespace and stripped non-alphanumeric
chars *after* tokenising. That made "well-tested" collapse to a
single token "welltested" while "well tested" stayed as two —
breaking the paraphrase cluster.

Splitting directly on `(c.is_whitespace() || !c.is_alphanumeric())`
turns "well-tested" into `["well", "tested"]`, which matches the
"well tested" paraphrase under Jaccard. Cost: empty-string tokens
between consecutive non-alphanumerics; filtered out at the bottom of
`tokens()`.

## Architecture

```
crates/mty-stdlib/src/swarm/
├── mod.rs         — `swarm()` entry point + `SwarmError` enum.
├── member.rs      — `Member` enum + `MemberReply` + `Member::ask()`.
├── consensus.rs   — `ConsensusStrategy` + `Consensus` + `resolve()`.
├── budget.rs      — `SharedDollarBudget` + `BudgetTripped`.
└── vote.rs        — `SimilarityMode`, `cluster_replies()`,
                     `similarity()`, internal token-set helpers.
```

`swarm() → run_first_agreed() / parallel_dispatch → consensus::resolve(replies, strategy, budget_exhausted)`
is the full call graph. Single-member panels skip both dispatchers
and short-circuit to `resolve()` with a one-element `replies` slice.

## Tests

| File | Count | Coverage |
|---|---|---|
| `crates/mty-stdlib/tests/swarm_basic.rs` | 7 | Empty panel, single member, three-agree-unanimous, split-majority, one-errored, all-errored, cost-collection |
| `crates/mty-stdlib/tests/swarm_budget.rs` | 6 | Under-budget, per-member-cost-decrement, FirstAgreed drops remaining, partial-with-flag, pre-exhausted, parallel-charges-all |
| `crates/mty-stdlib/tests/swarm_consensus.rs` | 10 | Majority strategy, Unanimous (disagree + agree), Weighted (explicit + defaults), FirstAgreed (short-circuit + disagree), token-set paraphrases, exact yes/no, majority ties |
| Module `#[cfg(test)]` blocks | 14 | Unit coverage for `Member::ask`, `SharedDollarBudget`, `cluster_replies`, every `ConsensusStrategy` |

All green:

```
cargo test -p mty-stdlib --test swarm_basic --test swarm_budget --test swarm_consensus
test result: ok. 7 passed; 0 failed
test result: ok. 6 passed; 0 failed
test result: ok. 10 passed; 0 failed
```

## What v0.28 picks up

- **Real OpenAI / Gemini / Bedrock clients.** Today their `Member::ask`
  routes through the v0.26 skeleton `complete()` (stub body). The
  swarm result shape doesn't change when those land — same
  `MemberReply`, same `Consensus`.
- **`SimilarityMode::Embedding`.** When `std.llm.embed` ships,
  clustering can use embed-cosine instead of Jaccard for richer
  paraphrase detection.
- **LLM-judge mode.** Optional path where Claude evaluates "do these
  N replies say the same thing" before clustering — useful for
  "answer is technically the same but wildly different prose"
  prompts.
- **Per-member typed `LlmBudget`.** Today the swarm uses a single
  `SharedDollarBudget`; v0.28 may add a per-member token budget on
  top so an "expensive frontier model on 10k tokens" path can
  short-circuit independently of "cheap small model on 200 tokens".

## What Track F can consume

- `swarm()` + `Member` + `ConsensusStrategy` + `SharedDollarBudget` +
  `Consensus` are public from `mty_stdlib::swarm`. Demos can build
  a panel without writing any new infrastructure.
- The Mighty-side `Member.anthropic(...)`, `ConsensusStrategy.Majority`,
  `.ask` are registered in `mty_types::prelude::build_prelude` as
  permissive methods, so `.mty` source compiles against the
  surface today.
- `Member::mock` + `Member::mock_error` are public for demo tests
  that want deterministic fixtures.
