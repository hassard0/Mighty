# `std.rag` — RAG-as-stdlib

> v0.33 Track T2 design notes. See also: `docs/reference/stdlib/rag.md`
> (user-facing surface), `docs/tour/21-rag-and-vision.md` (tour chapter).

## Why ship RAG as part of the stdlib

Every Mighty agent developer's first real project is RAG (retrieval-
augmented generation). Pre-v0.33 they had to wire the
`std.memory.VectorStore` plus a custom retrieval loop themselves —
the same boilerplate over and over, with subtly different chunking
strategies, different prompt-shaping conventions, no shared budget
discipline across the retrieve/rerank/answer trio.

`std.rag` promotes the v0.26 Track C vector primitive into a one-liner
pipeline:

```mty
let answer = Rag.new()
  .with_index(index)
  .with_retriever_top_k(5)
  .with_member(Member.anthropic("claude-opus-4-7"))
  .ask("What's Mighty's capability typing?")
```

## Module layout

```
crates/mty-stdlib/src/rag/
├── mod.rs              # public surface + re-exports
├── doc.rs              # Doc — source unit (text + metadata)
├── chunking.rs         # Chunker — 4 strategies
├── index.rs            # Index — wraps VectorStore + chunker
├── retriever.rs        # Retriever — kNN + min_score + MMR
├── reranker.rs         # Reranker — LLM-as-reranker
└── pipeline.rs         # Rag — end-to-end glue
```

Each module is independently testable: chunking has 11 unit tests,
index 7, retriever 5, reranker 5 (+ 1 parser), pipeline 9.
`tests/llm_multimodal.rs` and `tests/rag_vision_e2e.rs` cover the
vision path end-to-end against wiremock.

## The four chunking strategies

The chunker is the single biggest decision in a RAG pipeline — get it
wrong and retrieval surfaces irrelevant near-misses on every query.
`ChunkStrategy` ships four canonical strategies:

| Strategy        | Splits on              | Best for           | Default? |
|-----------------|------------------------|--------------------|----------|
| `ByTokens`      | Fixed approx-token windows + overlap | Catch-all when corpus shape is unknown | no |
| `ByParagraph`   | Blank lines, then merge under cap    | Prose / docs       | **yes**  |
| `BySection`     | Markdown headings (`#`, `##`, `###`) | Wikis / spec docs  | no |
| `ByCodeFence`   | Triple-backtick fences               | Tutorials / examples | no |

All four respect the same soft token cap (default 1024 tokens, where
"tokens" means whitespace-delimited words — a 25%-ish approximation
of a real BPE tokenizer's count). Callers who want exact counts plug
in `Chunker::with_token_counter(Arc<dyn Fn(&str) -> usize>)`.

Atomic units larger than the cap (a single oversized paragraph, a
single code fence) are emitted intact rather than silently truncated.
The alternative — dropping content the chunker can't fit — is the
classic "retrieval just refuses to find that thing" footgun.

## The two-phase add/build pattern

`Index::add_text` / `add_file` / `add_doc` stage docs into a pending
buffer. `Index::build` drains the buffer, chunks each doc, embeds
each chunk, and upserts into the underlying `VectorStore`.

The split is deliberate:

1. Real-world corpora have hundreds-to-thousands of docs. Embedding
   one chunk at a time burns network round-trips for the OpenAI
   embedder; the staging pattern lets a future embedder batch.
2. Re-adding a doc with the same id purges every prior chunk before
   re-chunking. Without the staging-then-build split this would
   force a separate `clear_doc(id)` call before every re-add.
3. Tests stay clean: `idx.add_text(...).add_file(...).build()?` reads
   top-to-bottom.

## The retriever

`Retriever` is a stateless borrow over an `Index`. Three knobs:

```mty
let r = Retriever.new(index)
  .with_top_k(5)
  .with_min_score(0.3)
  .with_mmr(true)
```

- `top_k` caps the result list.
- `min_score` drops hits below a cosine-similarity floor. Use this to
  prevent the "we have no relevant context but we'll happily answer
  anyway" failure mode — a min_score around 0.2-0.3 surfaces an empty
  hit list rather than 5 unrelated documents.
- `mmr` (Maximal Marginal Relevance) diversifies the top-k. When the
  raw cosine ranking has 5 near-duplicate hits, MMR picks 5 distinct
  ones. Implementation uses greedy MMR with lambda=0.5 and lexical
  Jaccard as the diversity proxy — no second embedding call needed.

## The reranker

`Reranker` is the optional LLM-as-reranker pass. Cheap embedding
retrieves 20-100 candidates; a smarter (slower, more expensive) LLM
re-scores each candidate on a 0-100 scale and the top-k is taken from
the re-scored list.

```mty
let rag = Rag.new()
  .with_index(index)
  .with_retriever_top_k(20)      // over-fetch for the reranker
  .with_reranker(Member.anthropic("claude-haiku-4-5"))
  .with_member(Member.anthropic("claude-opus-4-7"))
```

The reranker is **soft-failure**: any provider error (rate limit,
budget tripped, parse failure) falls back to the original cosine
scores. The pipeline never errors out because the reranker had a bad
day — the user gets a slightly-less-relevant answer instead of no
answer.

Mock-friendly: `Reranker` accepts any `Member` including `Member.mock`,
so the entire RAG pipeline can be exercised in tests without a real
provider.

## The shared budget

`Rag` carries one `SharedDollarBudget` across the reranker + answer
calls. Default cap is $1.00 per `ask`; tighten via
`.with_budget_cents(50)`. The reranker burns its budget first; if it
exhausts the cap the answer call sees `LlmError::BudgetExhausted`
rather than running unboundedly.

The budget is intentionally **per-pipeline**, not per-call. A long
agent session that issues 100 `rag.ask(...)` calls can share one
$10 cap across them all by constructing one `SharedDollarBudget(1000)`
and passing it via `with_budget`.

## Multi-modal usage

`std.rag` exposes two image-aware methods on `Rag`:

```mty
rag.ask_with_image(query, Image.from_file("./diagram.png"))
rag.ask_with_images(query, [img1, img2])
```

The pipeline runs the same retrieve-then-rerank-then-answer flow; the
image rides on the answering turn as a sibling `ContentBlock::Image`
to the augmented prompt's text block. Each provider's wire shape is
handled by `mty_stdlib::llm::*::message_to_*`:

| Provider   | Shape                                               |
|------------|-----------------------------------------------------|
| Anthropic  | `{type: "image", source: {type: "base64", media_type, data}}` |
| OpenAI     | `{type: "input_image", image_url: "data:image/...;base64,..."}` |
| Gemini     | `{inlineData: {mimeType, data}}`                    |
| Bedrock    | `{image: {format, source: {bytes}}}`                |

URLs pass through verbatim where the provider accepts them
(Anthropic / OpenAI / Gemini); Bedrock falls back to a text
`[image: <url>]` stand-in because the Converse API rejects URL
sources.

## `std.llm.Image`

The companion type that built-in vision uses:

```mty
let img = Image.from_file("./pic.jpg")    // cap fs.read; auto mime
let img = Image.from_bytes(bytes, "image/png")  // no cap (caller had bytes)
let img = Image.from_url("https://example.com/a.png")  // cap net.https
```

Mime-type detection is from the file extension; falls back to
`image/png` when missing. Bytes ≤ a few MB are base64-encoded inline
at `to_source()` time (called by `Rag::ask_with_image` and by the
provider serialisers). The base64 encoder is vendored inline in
`mty_stdlib::llm::image::base64_encode` — RFC 4648 §4 standard
alphabet, padded; the implementation is ~25 lines and trivially
auditable, avoiding a dep on `base64` for one function.

## Replay determinism

`std.rag` inherits the deterministic-replay contract from
`std.memory.VectorStore` (v0.26 Track C): every `Index::build` upsert
records a `MemoryDelta::Patch` event in the v0.19 trace, so `mty
replay` reconstructs the same index state at any frame. The reranker
+ answer calls go through `Member::ask` which already records via
`record_member_turn` — same path the swarm primitive uses.

The chunker is fully deterministic — same input doc produces the same
chunk ids, texts, and order on every run. The stub embedder is L2-
normalised FNV-hash sums, also deterministic across platforms.

## When NOT to use `std.rag`

- **Tiny corpora (<10 docs)**: just stuff everything into the prompt.
  RAG overhead (chunking + embedding + retrieval) costs more than the
  context-window savings.
- **Real-time conversational memory**: use `std.memory.Episodic` for
  the timeline + `std.memory.Working` for the scratchpad. RAG is for
  ground-truth retrieval, not for "what did the user say 2 turns ago".
- **Structured-data lookup**: if the answer is in a SQL row, use SQL.
  RAG over a 10M-row table is strictly worse than `SELECT ... WHERE`.

## Future work (v0.34+)

- **Hybrid retrieval**: combine cosine + BM25 scores. The chunker
  already exposes the chunk text, so a sparse-vector path is purely
  additive.
- **Streaming `ask_stream`**: the answering Member's `complete_stream`
  is already wired; only the pipeline layer needs to surface a
  streaming variant.
- **Persistent reranker cache**: the reranker scores the same
  (query, chunk) pair the same way on every call. A keyed cache on
  the `(query_hash, chunk_id)` tuple would let warm queries skip the
  reranker entirely.
- **Query rewriting**: a small "expand my query into 3 variants" step
  before retrieval often surfaces 2x the recall. Slots in between
  `embed` and `search` without touching the rest of the pipeline.

## Cross-references

- `std.memory.VectorStore` — `docs/reference/stdlib/memory.md`
- `std.swarm.Member` — `docs/reference/stdlib/swarm.md`
- `std.llm.Image` — `docs/reference/stdlib/llm.md` (multi-modal section)
- Demo 10 — `demos/10_vision_rag/` (vision-RAG forcing function)
- Tour chapter — `docs/tour/21-rag-and-vision.md`
