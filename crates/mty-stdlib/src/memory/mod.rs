//! `std.memory` — vector + episodic + working memory primitives for
//! LLM-agent workflows (v0.26 Track C).
//!
//! The module ships three concrete handle types that share a tiny
//! [`MemoryHandle`] trait so the snapshot integration in
//! [`snapshot`] can treat any backend uniformly:
//!
//! - [`VectorStore`](vector::VectorStore) — semantic search over text
//!   chunks; local (in-memory + JSON-on-disk) or qdrant HTTP.
//! - [`Episodic`](episodic::Episodic) — append-only timeline of
//!   `(timestamp, key, value)`; in-memory ring buffer or sqlite-backed.
//! - [`Working`](working::Working) — bounded scratchpad with a
//!   markdown render for prompt assembly.
//!
//! ## Surface example
//!
//! ```ignore
//! agent Researcher {
//!   vector: VectorStore = VectorStore.local("./mem.qdrant")
//!   episodic: Episodic = Episodic.in_memory(max: 100)
//!   working: Working = Working.new()
//!
//!   on Query(q: String) -> String {
//!     let recall = self.vector.search(q, k: 5)
//!     let answer = anthropic.messages(...).await
//!     self.episodic.record(q, answer)
//!     answer
//!   }
//! }
//! ```
//!
//! ## Replay determinism
//!
//! Every mutation to a memory handle is recorded through
//! [`snapshot::record_memory_delta`] as a [`MemoryDelta`] event in the
//! v0.19 trace. `mty replay` walks the trace and reconstructs the same
//! handle state at any frame — the snapshot bytes themselves are
//! deterministic (sorted keys, fixed encoding) so the replay byte-
//! identical contract extends to `std.memory`.
//!
//! ## Feature flags
//!
//! - `memory-sqlite` (default-on) — pulls in `rusqlite` for
//!   [`Episodic::sqlite`](episodic::Episodic::sqlite). Disable to keep
//!   the dep graph minimal on no-libc targets.
//! - `memory-openai` (off by default) — opt-in OpenAI embeddings; the
//!   default test harness uses the deterministic stub embedder.
//! - `memory-qdrant` (off by default) — opt-in qdrant HTTP backend; the
//!   default local backend has no network dependency.

pub mod embeddings;
pub mod episodic;
pub mod snapshot;
pub mod vector;
pub mod working;

pub use embeddings::{Embedder, EmbeddingErr, StubEmbedder};
pub use episodic::{Entry, Episodic, EpisodicErr};
pub use snapshot::{MemoryDelta, SnapshotBytes};
pub use vector::{Hit, VectorErr, VectorStore};
pub use working::{Working, WorkingEntry};

/// Common shape implemented by every concrete memory handle. The
/// snapshot integration uses this trait to round-trip handle state
/// into the v0.19 replay trace.
///
/// The trait is intentionally minimal — concrete handles expose
/// richer APIs (search, ring-buffer semantics, render-to-markdown)
/// via their own inherent methods; `MemoryHandle` is the lowest
/// common denominator the replay layer needs.
pub trait MemoryHandle {
    /// Stable string identifying the backend kind (`"vector.local"`,
    /// `"vector.qdrant"`, `"episodic.in_memory"`,
    /// `"episodic.sqlite"`, `"working"`). Used as the discriminator
    /// when the snapshot is later restored.
    fn kind(&self) -> &'static str;

    /// Capture the current state into a portable byte snapshot. The
    /// encoding is deterministic — calling `snapshot()` twice with no
    /// intervening mutation returns byte-identical buffers.
    fn snapshot(&self) -> SnapshotBytes;

    /// Restore from a snapshot previously produced by [`snapshot`].
    /// Implementations should reject malformed input by returning a
    /// `SnapshotBytes` length mismatch / decode error.
    fn restore(&mut self, snapshot: &SnapshotBytes) -> Result<(), String>;
}
