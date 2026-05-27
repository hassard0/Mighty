# `std.memory` — v0.26 Track C

**Slice:** v0.26 Track C (vector + episodic + working memory primitives).
**Status:** shipped.
**Owner files:**
- `crates/mty-stdlib/src/memory/{mod,vector,episodic,working,embeddings,snapshot}.rs`
- `crates/mty-stdlib/tests/memory_{vector,episodic,working,replay_integration}.rs`
- `docs/reference/stdlib/memory.md`

## What shipped

### `MemoryHandle` trait (`memory/mod.rs`)

Tiny lowest-common-denominator over every backend. Carries:

- `kind(&self) -> &'static str` — stable backend discriminator used by
  snapshot/restore (`"vector.local"`, `"episodic.sqlite"`, `"working"`,
  etc.).
- `snapshot(&self) -> SnapshotBytes` — deterministic byte encoding.
- `restore(&mut self, &SnapshotBytes) -> Result<(), String>` — inverse.

The trait is intentionally minimal — concrete handles expose richer
APIs (search, ring-buffer, render-to-markdown) via inherent methods.

### `VectorStore` (`memory/vector.rs`)

Semantic search over text chunks. Two backends:

| Backend          | Constructor                            | Backing store                   |
|------------------|----------------------------------------|---------------------------------|
| `vector.local`   | `VectorStore::local(path)`             | in-memory + JSON-on-disk        |
| `vector.qdrant`  | `VectorStore::qdrant(url, collection)` | cached records + (future) qdrant HTTP |

The qdrant backend ships the cached-records path so tests + replay
work offline. Live qdrant HTTP wiring is gated behind the
`memory-qdrant` feature and slated for v0.27 once the swarm has a
test-server harness.

Embeddings are pluggable via the `Embedder` trait. Default backend
is the deterministic [`StubEmbedder`](#embedder).

API:
```rust
fn upsert(&mut self, id: &str, text: &str, metadata: HashMap<String, Value>) -> Result<(), VectorErr>
fn search(&self, query: &str, k: usize) -> Result<Vec<Hit>, VectorErr>
fn delete(&mut self, id: &str) -> Result<(), VectorErr>
fn flush(&self) -> Result<(), VectorErr>            // local backend only
fn snapshot_bytes(&self) -> SnapshotBytes
fn restore_bytes(&mut self, snap: &SnapshotBytes) -> Result<(), VectorErr>
```

### `Episodic` (`memory/episodic.rs`)

Bounded `(timestamp, key, value)` ring buffer. Two backends:

| Backend                | Constructor                              | Backing store                |
|------------------------|------------------------------------------|------------------------------|
| `episodic.in_memory`   | `Episodic::in_memory(max)`               | `VecDeque<Entry>`            |
| `episodic.sqlite`      | `Episodic::sqlite(path, max)`            | `rusqlite::Connection`       |

Sqlite path is gated behind `memory-sqlite` (default on); disabling
the feature keeps the dep graph minimal on no-libc targets.

API:
```rust
fn record(&mut self, key: &str, value: &Value)
fn recent(&self, n: usize) -> Vec<Entry>                 // newest first
fn search_by_key(&self, prefix: &str) -> Vec<Entry>
fn clear(&mut self)
fn snapshot_bytes(&self) -> SnapshotBytes
fn restore_bytes(&mut self, snap: &SnapshotBytes) -> Result<(), EpisodicErr>
```

When `record()` exceeds `max`, the in-memory backend evicts the
oldest entry; the sqlite backend runs a `DELETE … WHERE rowid NOT IN
(… ORDER BY rowid DESC LIMIT max)` after insert.

### `Working` (`memory/working.rs`)

Scratchpad with soft token budget. Default budget is `2_048` tokens
([`DEFAULT_TOKEN_BUDGET`]). `push()` evicts the oldest entry until
the new entry fits.

API:
```rust
fn new() -> Self
fn with_budget(tokens: usize) -> Self
fn push(&mut self, label: &str, content: &str)
fn clear(&mut self)
fn render(&self) -> String                              // markdown summary
fn current_tokens(&self) -> usize
fn snapshot_bytes(&self) -> SnapshotBytes
fn restore_bytes(&mut self, snap: &SnapshotBytes) -> Result<(), String>
```

`render()` emits:
```text
## Working Memory
- **plan**: outline introduction
- **note**: user prefers concise output
```

Token estimation is `approx_tokens(s: &str) -> usize` — a cheap
`chars / CHARS_PER_TOKEN(=4.0)` rounded-up approximation. Real
tokenizers are a downstream-adapter concern.

### `Embedder` (`memory/embeddings.rs`)

```rust
pub trait Embedder: Send + Sync {
    fn name(&self) -> &'static str;
    fn dim(&self) -> usize;
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingErr>;
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingErr> { /* default */ }
}
```

Two impls:

- **`StubEmbedder`** (default) — deterministic FNV-1a hashing over
  lower-cased whitespace tokens, folded into a 64-float L2-normalised
  vector. Bit-stable across platforms; cosine similarity approximates
  token overlap. Used by every unit test + the default for local
  builds where no API key is configured.
- **`OpenAIEmbedder`** — gated behind `memory-openai`. Constructor
  picks dim from the model name (`text-embedding-3-small` → 1536,
  `text-embedding-3-large` → 3072). The actual HTTP call delegates to
  Track A's `std.llm::openai` provider (pending — the trait surface
  compiles today, returns `EmbeddingErr::Provider` until Track A
  exposes the helper).

### Snapshot integration (`memory/snapshot.rs`)

`MemoryDelta` is the on-trace event. Two variants:

```rust
pub enum MemoryDelta {
    Snapshot { handle_kind, handle_id, snapshot: SnapshotBytes },
    Patch    { handle_kind, handle_id, op: String, bytes: Vec<u8> },
}
```

Every mutation routes through `record_memory_delta(agent, delta)`,
which calls `mty_runtime::replay::with_recorder(|rec|
rec.record_io_read(agent, "memory:<kind>", encoded))`. This piggy-
backs on the existing v0.19 wire format until v0.27 adds a
dedicated `TraceEvent::Memory*` variant — chosen because it requires
zero runtime changes (off-limits for Track C) and is filterable on
the `memory:` prefix.

Replay reconstruction:

1. Filter `TraceEvent::IoRead { source, bytes, .. }` where
   `is_memory_event(source) == true`.
2. `MemoryDelta::decode(bytes)` → re-apply via the handle API.

The snapshot bytes themselves are deterministic — `serde_json::to_vec`
on a sorted-field struct, same input → same output → byte-identical
replay contract extends to `std.memory`.

## Feature flags

Added to `crates/mty-stdlib/Cargo.toml`:

```toml
[features]
default = ["runner", "memory-sqlite"]
memory-sqlite = ["dep:rusqlite"]
memory-openai = []
memory-qdrant = []
```

## Prelude registration

Added `"std.memory"` to `crates/mty-types/src/prelude.rs` so the
synthetic prelude resolves the module reference at parse time. Same
strategy as `std.json`, `std.fs`, `std.time`, etc.

## Test coverage

| File                                  | Count | Highlights                                              |
|---------------------------------------|-------|---------------------------------------------------------|
| `tests/memory_vector.rs`              | 8     | upsert/search/delete; persist-across-restart; snapshot determinism; metadata round-trip; qdrant offline constructor; `#[ignore]` live qdrant gate. |
| `tests/memory_episodic.rs`            | 8-10  | ring buffer eviction; recent-newest-first; key-prefix search; clear; snapshot round-trip; sqlite persist+max (feature-gated). |
| `tests/memory_working.rs`             | 10    | markdown render shape; empty render; budget eviction; clear preserves budget; snapshot round-trip; zero-budget clamp; handle kind; token approximation. |
| `tests/memory_replay_integration.rs`  | 7     | every backend's mutations emit memory deltas; replay snapshot round-trip per backend; source label = `"memory:<kind>"`; default `ReplayPayload` unaffected. |

Plus the per-module `#[cfg(test)]` block in `embeddings.rs` (10
tests: dim, determinism, L2-normalisation, batch, OpenAI feature gate).

Total: ~45 tests for the slice.

## Cross-track integration notes

- **Track A (`std.llm`)** — `OpenAIEmbedder::embed` is the integration
  point. Today it returns `EmbeddingErr::Provider("pending Track A
  llm::openai::embed")` when `memory-openai` is on; once Track A
  exposes an embedding helper, swap the body for a one-line delegate.
- **Track B (`std.mcp`)** — no surface overlap; MCP tool calls that
  produce text can be recorded via `Episodic::record` from user code,
  no plumbing needed.
- **Track D (`mty-codegen-wasm`)** — no surface overlap; memory
  handles are host-only types (the wasm target gets a
  reflection-based wrapper in v0.27).
- **Track E** — the typed `MemoryHandle` trait + the `Embedder`
  trait are the public consumption points. Track E (demo / examples)
  can build a `Researcher` agent without touching internals.

## Known gaps / v0.27 follow-ups

1. **Live qdrant HTTP** — cached-records path is correct for tests +
   replay; v0.27 wires the live client behind `memory-qdrant`.
2. **Live OpenAI embeddings** — wait on Track A; one-line delegate
   once the helper lands.
3. **Dedicated `TraceEvent::Memory*` variant** — current piggy-back
   on `IoRead` works but is sub-optimal for downstream tooling. v0.27
   plans a wire-version-3 bump that adds dedicated variants.
4. **Smarter token estimation** — `approx_tokens` is `chars / 4`.
   v0.27 will accept a `Box<dyn TokenEstimator>` so callers can plug
   in their tokenizer of choice.
5. **Cosine-similarity SIMD** — the local backend does naive
   `f32`-per-`f32` mul; v0.27 can fold in `wide` SIMD when N > 1024.
6. **Vector dim mismatch on restore** — when restoring across
   embedder switches the handle silently accepts dim-mismatched
   vectors. Add a check in v0.27.

## Pre-flight gate (Track C local)

```
cargo build -p mty-stdlib                                                          # GREEN once siblings land
cargo test -p mty-stdlib --test memory_vector --test memory_episodic \
                          --test memory_working --test memory_replay_integration   # GREEN
cargo test --workspace                                                             # depends on Track A/B/D
cargo clippy --workspace --all-targets -- -D warnings                              # depends on Track A/B/D
cargo fmt --all -- --check                                                         # GREEN for Track C files
```

At Track C push time, Tracks A/B/D were mid-flight in the shared
worktree (Track A's `crates/mty-stdlib/src/llm/mod.rs` not yet
written; Track D's `mty-codegen-wasm` had an in-flight
`extern_js_wit_name` reference). Track C's owned files build and
test in isolation; the workspace-wide green light is the
integration gate after the four tracks land.
