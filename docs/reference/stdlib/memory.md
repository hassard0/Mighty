# `std.memory`

Vector + episodic + working memory primitives for LLM-agent
workflows. Shipped in v0.26 Track C alongside Track A's `std.llm`
and Track B's `std.mcp` — the three modules together cover the
"agent talks to a model, remembers what it did" loop.

Every memory handle integrates with the v0.19 deterministic-replay
trace: each mutation emits a `MemoryDelta` event, so `mty replay`
reconstructs the same in-memory state at any frame.

## Surface

```sd
use std.memory

agent Researcher {
  vector: VectorStore = VectorStore.local("./mem.qdrant")
  episodic: Episodic  = Episodic.in_memory(max: 100)
  working: Working    = Working.new()

  on Query(q: String) -> String {
    let recall = self.vector.search(q, k: 5)
    let answer = anthropic.messages(...).await
    self.episodic.record(q, answer)
    answer
  }
}
```

## `VectorStore`

Semantic search over text chunks. Two backends ship in v0.26:

| Constructor                          | Backend                              | Persistence              |
|--------------------------------------|--------------------------------------|--------------------------|
| `VectorStore.local("./path.json")`   | in-memory + JSON-on-disk             | full, on every write     |
| `VectorStore.qdrant(url, collection)`| HTTP qdrant client                   | qdrant-side              |

Embeddings flow through an `Embedder` trait. The default (used by
both backends + every unit test) is the deterministic
[`StubEmbedder`](#stubembedder) — bit-stable across runs, zero
network. Plug in an OpenAI `text-embedding-3-*` embedder via the
`memory-openai` feature.

### Methods

```sd
fn upsert(id: Str, text: Str, metadata: Map[Str, Json]) -> Unit!VectorErr
fn search(query: Str, k: Size) -> Vec[Hit]!VectorErr
fn delete(id: Str) -> Unit!VectorErr
fn flush() -> Unit!VectorErr                  // local backend only
fn snapshot_bytes() -> SnapshotBytes
fn restore_bytes(snap: SnapshotBytes) -> Unit!VectorErr
```

A `Hit` is `{ id, text, score, metadata }` — score is cosine
similarity in `[-1.0, 1.0]`, sorted descending.

## `Episodic`

Append-only `(timestamp, key, value)` ring buffer. Two backends:

| Constructor                          | Backend                              | Persistence              |
|--------------------------------------|--------------------------------------|--------------------------|
| `Episodic.in_memory(max: 100)`       | `VecDeque` ring                      | none                     |
| `Episodic.sqlite("./ep.sqlite", max)`| `rusqlite`                           | sqlite file              |

The sqlite path is gated behind the `memory-sqlite` feature
(default-on). Disable to keep the dep graph minimal on no-libc
targets.

### Methods

```sd
fn record(key: Str, value: Json) -> Unit
fn recent(n: Size) -> Vec[Entry]                 // newest first
fn search_by_key(prefix: Str) -> Vec[Entry]
fn clear() -> Unit
fn snapshot_bytes() -> SnapshotBytes
fn restore_bytes(snap: SnapshotBytes) -> Unit!EpisodicErr
```

When `record()` would exceed `max` the oldest entry is evicted
(in-memory) or the rowid-window is enforced via `DELETE` (sqlite).

## `Working`

Scratchpad with a soft token budget. Default budget is `2_048` tokens
(see [`DEFAULT_TOKEN_BUDGET`]).

### Methods

```sd
fn new() -> Working                              // default budget
fn with_budget(tokens: Size) -> Working
fn push(label: Str, content: Str) -> Unit
fn clear() -> Unit
fn render() -> String                            // markdown summary
fn current_tokens() -> Size
fn snapshot_bytes() -> SnapshotBytes
fn restore_bytes(snap: SnapshotBytes) -> Unit!String
```

`render()` produces:

```text
## Working Memory
- **plan**: outline the introduction
- **note**: user prefers concise output
```

When a `push()` would exceed `token_budget`, the oldest entries are
dropped one by one until the new entry fits.

### Token counting

[`approx_tokens`] is a cheap `chars.len() / 4` rounded-up estimate
— good enough for budget enforcement, *not* a real tokenizer.
Production code should swap in a real tokenizer via a downstream
adapter (the type accepts any string-based estimate, no enforcement).

## `StubEmbedder`

The default embedder. Deterministic FNV-1a hashing over
lower-cased whitespace tokens, folded into a 64-float vector and
L2-normalised. Cosine similarity approximates token overlap.

- Bit-stable across platforms.
- Offline — no network call.
- **Not semantic** — two synonyms with no shared tokens land
  orthogonal. For real semantic search use the OpenAI embedder
  (`memory-openai` feature).

## `Embedder` trait

```sd
trait Embedder {
  fn name() -> Str
  fn dim() -> Size
  fn embed(text: Str) -> Vec[F32]!EmbeddingErr
  fn embed_batch(texts: Vec[Str]) -> Vec[Vec[F32]]!EmbeddingErr
}
```

Custom embedders implement the trait + plug in via
`VectorStore.with_embedder(Arc<dyn Embedder>)`.

## Snapshot + replay

Every mutation on every handle emits a `MemoryDelta::Patch` event
into the process-wide v0.19 trace, routed through the recorder's
`record_io_read` hook with source label `memory:<handle_kind>`.

```sd
enum MemoryDelta {
  Snapshot { handle_kind, handle_id, snapshot },
  Patch    { handle_kind, handle_id, op, bytes },
}
```

`mty replay --dump-json` writes one JSON object per event so a
post-hoc walker can rebuild handle state by either:

1. Replaying every `Patch` in order (cheap delta encoding).
2. Reading the last `Snapshot` event before the target frame (always works).

The snapshot bytes themselves are deterministic — calling
`snapshot_bytes()` twice with no intervening mutation returns
byte-identical buffers, which extends the v0.19 byte-identical replay
contract to `std.memory`.

## Feature flags

| Feature           | Default | Effect                                       |
|-------------------|---------|----------------------------------------------|
| `memory-sqlite`   | on      | `Episodic::sqlite(...)` actually opens a DB. |
| `memory-openai`   | off     | `OpenAIEmbedder::embed` performs HTTP.       |
| `memory-qdrant`   | off     | `VectorStore::qdrant` performs HTTP.         |

## See also

- `docs/reference/stdlib/llm.md` — Track A providers (`anthropic`, `openai`, ...)
- `docs/reference/stdlib/mcp.md` — Track B Model Context Protocol bridge
- `docs/reference/cli/mty-replay.md` — `mty replay` trace inspector
