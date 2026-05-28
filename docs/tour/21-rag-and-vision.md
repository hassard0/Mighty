# Chapter 21 — RAG and vision

> v0.33 Track T2. By the end of this chapter you'll have built a
> grounded question-answering agent that can read an image, retrieve
> relevant context from your own corpus, and answer with citations.

## The one-liner RAG pipeline

Mighty's standard library ships a complete RAG (retrieval-augmented
generation) pipeline behind one chained constructor:

```mty
use std.rag.{Index, Rag}
use std.swarm.Member

let mut index = Index.new("./corpus")
index.add_text("Mighty is...", {source: "intro"})
index.add_file("./docs/spec.md")
let _ = index.build()

let rag = Rag.new()
  .with_index(index)
  .with_retriever_top_k(5)
  .with_member(Member.anthropic("claude-opus-4-7"))

let answer: Str = rag.ask("What's Mighty's capability typing?")
```

Five lines from "I have a corpus" to "I have a grounded answer".
That's the v0.33 Track T2 mandate: every agent dev's first real
project (RAG) is now a stdlib one-liner.

## What's happening under the hood

`Rag.ask(query)` runs:

1. **Embed** the query through the index's embedder.
2. **Retrieve** the top-k hits via kNN cosine similarity.
3. **Rerank** (optional) — re-score hits with an LLM.
4. **Augment** the prompt with the retrieved context.
5. **Ask** the answering `Member` and return the body.

The retrieval layer is a `Retriever` over an `Index`. The index wraps
a `VectorStore` plus a `Chunker`. You can swap any of these without
rewriting the others.

## Picking a chunking strategy

The chunker is the single biggest decision. Four strategies ship:

```mty
use std.rag.{Chunker, ChunkStrategy}

let chunker = Chunker.new(ChunkStrategy.ByParagraph)  // default
let chunker = Chunker.new(ChunkStrategy.ByTokens)     // sliding window
let chunker = Chunker.new(ChunkStrategy.BySection)    // markdown # headings
let chunker = Chunker.new(ChunkStrategy.ByCodeFence)  // backtick fences

let mut index = Index.new("./corpus").with_chunker(chunker)
```

Default is `ByParagraph` with a 1024-token soft cap and 64-token
overlap — good catch-all for prose docs. Switch to `BySection` for
wiki-shaped corpora and `ByCodeFence` for tutorial-shaped ones.

## Adding a reranker

When retrieval surfaces 5 near-duplicates and you want the actual top-5
by relevance, add a reranker:

```mty
let rag = Rag.new()
  .with_index(index)
  .with_retriever_top_k(20)                          // over-fetch
  .with_reranker(Member.anthropic("claude-haiku-4-5")) // cheap LLM
  .with_member(Member.anthropic("claude-opus-4-7"))    // smart LLM
```

The reranker is **soft-failure**: if it errors or trips its budget the
pipeline falls back to the original cosine scores. You never end up
with no answer because the reranker had a bad day.

## Multi-modal: ask with an image

Every provider that Mighty wraps (Anthropic / OpenAI / Gemini /
Bedrock) supports image input in v0.33. The RAG pipeline lifts it into
a one-liner:

```mty
use std.llm.Image

let diagram = Image.from_file("./architecture.png")
let answer: Str = rag.ask_with_image(
  "What does this architecture diagram show?",
  diagram,
)
```

The image rides on the answering turn as a sibling content block to
the augmented prompt. Retrieval is still text-only (the query string
is what gets embedded); the image gives the LLM grounding the corpus
alone can't.

For multiple images, use `ask_with_images([img1, img2])`.

## Constructing `Image` values

Three constructors, each with the right capability:

```mty
Image.from_file("./pic.jpg")                    // cap fs.read
Image.from_bytes(bytes, "image/png")            // no cap (you had bytes)
Image.from_url("https://example.com/a.webp")    // cap net.https
```

Mime-type detection is automatic from file extension. Supported:
PNG / JPG / JPEG / GIF / WebP. Anything else falls back to
`image/png`.

## A full vision-RAG agent

`demos/10_vision_rag/src/main.mty` is the canonical forcing-function
demo for this chapter. Strip its boilerplate down to:

```mty
package vision_rag

use std.llm
use std.rag
use std.swarm
use std.env

protocol VisionInput {
  Ask(question: Str, diagram_path: Str) -> Str
}

agent VisionResearcher: VisionInput {
  on Ask(question, diagram_path) {
    let mut index = Index.new("./corpus")
    index.add_file("./corpus/intro.md")
    index.add_file("./corpus/spec.md")
    let _ = index.build()

    let rag = Rag.new()
      .with_index(index)
      .with_retriever_top_k(3)
      .with_member(Member.anthropic("claude-opus-4-7"))

    let diagram = Image.from_file(diagram_path)
    let answer: Str = rag.ask_with_image(question, diagram)
    answer
  }
}

fn main() {
  let researcher = spawn VisionResearcher()
  let argv = std.env.args()
  let path = argv.get(0).unwrap_or("./diagram.png")
  let q = argv.get(1).unwrap_or("What does this show?")
  let answer: Str = researcher ! Ask(q, path)
  log(answer)
}
```

Run it:

```
ANTHROPIC_API_KEY=sk-ant-... mty run \
    demos/10_vision_rag/src/main.mty -- ./diagram.png "Summarise this"
```

## What about budgets?

Both the reranker and the answer call go through `Member.ask`, which
charges a `SharedDollarBudget`. By default `Rag` caps every `ask` at
$1.00. Tighten:

```mty
let rag = Rag.new()
  .with_budget_cents(25)   // 25 cents per ask
  .with_index(index)
  .with_member(member)
```

Or share one budget across many asks:

```mty
let budget = DollarBudget.from_dollars(5.00)
let rag = Rag.new().with_budget(budget).with_index(index).with_member(m)

for q in questions {
  let _ = rag.ask(q)   // all 5 dollars shared across the whole loop
}
```

## Replay determinism

Every `Index.build` records a `MemoryDelta::Patch` in the v0.19
trace, and every `Member.ask` records its turn through the same
recorder the swarm primitive uses. `mty replay` reconstructs the
identical index state + identical LLM responses at any frame — you
can re-debug a vision-RAG turn from production by replaying the
trace, no live API calls.

## Where to go from here

- Chapter 17 — Swarm + eval (the panel-of-members primitive that
  `Rag.with_reranker` is built on).
- Chapter 19 — Observability (`std.observe` records every LLM call
  through `Rag.ask` automatically).
- `docs/internals/rag.md` — design rationale, the four chunking
  strategies in depth, multi-modal wire shapes per provider.
- `demos/10_vision_rag/` — the forcing-function demo this chapter
  walks through.
