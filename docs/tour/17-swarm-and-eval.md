# Chapter 17 — Swarm consensus and `std.eval`

Examples:
[`31_eval_agent.mty`](https://github.com/hassard0/Mighty/blob/main/examples/31_eval_agent.mty),
[`32_eval_native.mty`](https://github.com/hassard0/Mighty/blob/main/examples/32_eval_native.mty),
plus demos
[`08_swarm_review`](https://github.com/hassard0/Mighty/blob/main/demos/08_swarm_review/README.md)
and
[`09_distributed_swarm`](https://github.com/hassard0/Mighty/blob/main/demos/09_distributed_swarm/README.md).

Two surfaces ship together because they compose: `std.swarm`
votes consensus across providers; `std.eval` regression-tests the
swarm's verdicts against a frozen panel under byte-identical
replay.

## `std.swarm` — multi-provider consensus

```mty
use std.swarm.{Member, ConsensusStrategy, DollarBudget, swarm}

fn review(snippet: Str) -> Str {
  let panel = vec![
    Member.anthropic(anthropic_client, "claude-opus-4-7"),
    Member.openai(openai_client, "gpt-5"),
    Member.gemini(gemini_client, "gemini-2.0-flash"),
  ];
  let budget = DollarBudget.from_dollars(0.50);
  let consensus = swarm(
    prompt = format!("Is this code safe? {}", snippet),
    panel = panel,
    budget = budget,
    strategy = ConsensusStrategy.Majority,
  );
  format!(
    "verdict={}, cost={c}c, dissents={d}",
    consensus.majority,
    c = consensus.total_cost_cents,
    d = consensus.dissents.len(),
  )
}
```

Strategies: `Majority`, `Plurality`, `Unanimous`,
`WeightedVote`, `FirstAgreed`. The `DollarBudget` is **shared**
across the panel and short-circuits when exhausted — `Consensus`
carries `budget_exhausted: true` so the caller can react.

See [reference/stdlib/swarm.md](../reference/stdlib/swarm.md).

## `std.eval` — LLM-agent regression harness

`std.eval` builds on **byte-identical replay** to make LLM-agent
tests deterministic in the same way unit tests on pure code are
deterministic. A `*.eval.mty` suite declares cases + a panel + a
comparison:

```mty
@suite("kvstore-routing")
fn kvstore_routing_suite() -> EvalSuite {
  EvalSuite::new()
    .add(Case::new("route-by-hash")
      .prompt("PUT alpha=1")
      .expect_contains("shard:1"))
    .panel(vec![
      Member.anthropic(client, "claude-opus-4-7"),
      Member.openai(client, "gpt-5"),
    ])
    .compare(Compare::ExpectAllPass)
}
```

Run it:

```bash
mty test --eval examples/31_eval_agent.mty
```

`mty test --eval` walks the suite's cases, fires the panel against
each case, and asserts the comparison. Under
`MTY_REPLAY=path/to/trace.json` the same suite re-runs against the
recorded trace — no LLM tokens spent, byte-identical PASS/FAIL.

The v0.29 Track F native replay backend (`Replay::with_provider`
+ `iter_llm_calls`) makes this fast enough for inner-loop work;
`mty test --eval --watch` is the v0.31 polish.

## Try it

```bash
mty check examples/31_eval_agent.mty
mty check examples/32_eval_native.mty
```

For a real run, see Demo 08 — it composes `std.swarm` into a
working multi-provider code reviewer with a 50¢ shared budget.
