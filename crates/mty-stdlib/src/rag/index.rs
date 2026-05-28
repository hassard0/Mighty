//! `std.rag.Index` — wraps a [`VectorStore`](crate::memory::VectorStore)
//! plus a [`Chunker`](super::Chunker) into the staging-then-build
//! pattern most RAG corpora want.
//!
//! ```ignore
//! let mut idx = Index.new("./corpus")
//! idx.add_text("...", {source: "intro"})
//! idx.add_file("./docs/spec.md")?
//! idx.build()?           // chunk every staged doc + embed + upsert
//! let hits = idx.search("question", 5)?
//! ```
//!
//! `Index` exposes both `search` (returning `Vec<Hit>` directly) and
//! the underlying `VectorStore` reference for advanced callers.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use serde_json::Value;
use thiserror::Error;

use crate::memory::embeddings::Embedder;
use crate::memory::vector::{Hit, VectorErr, VectorStore};

use super::chunking::{ChunkStrategy, Chunker};
use super::doc::Doc;

/// Errors returned by [`Index`].
#[derive(Debug, Error)]
pub enum IndexErr {
    #[error("rag.Index io: {0}")]
    Io(String),
    #[error("rag.Index vector backend: {0}")]
    Vector(#[from] VectorErr),
}

impl From<std::io::Error> for IndexErr {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// Mutable RAG index. Holds staged docs and an underlying
/// [`VectorStore`].
pub struct Index {
    chunker: Chunker,
    store: VectorStore,
    /// Docs added via `add_*` but not yet built into the store.
    /// `build()` drains this.
    pending: Vec<Doc>,
    /// Track every doc id we've ever built so re-`add` of the same id
    /// can purge prior chunks from the store.
    built_doc_ids: Vec<String>,
}

impl std::fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Index")
            .field("chunker", &self.chunker)
            .field("pending", &self.pending.len())
            .field("built_docs", &self.built_doc_ids.len())
            .field("store_len", &self.store.len())
            .finish()
    }
}

impl Index {
    /// New disk-backed index at `path`. The path holds the underlying
    /// [`VectorStore`] JSON. `path` may not exist yet — `build` creates
    /// the parent dir if needed.
    pub fn new(path: impl AsRef<Path>) -> Self {
        let store = VectorStore::local(path.as_ref().join("vectors.json"));
        Self {
            chunker: Chunker::default(),
            store,
            pending: Vec::new(),
            built_doc_ids: Vec::new(),
        }
    }

    /// Pure in-memory index — useful for tests + ephemeral RAG over a
    /// short corpus. No disk persistence.
    pub fn in_memory() -> Self {
        // Use a placeholder path with the local backend so we get the
        // in-memory + JSON-on-disk pair. We point at a temp-ish path
        // under the OS temp dir derived from the process id; this keeps
        // round-trip semantics correct without forcing the caller to
        // pick a directory.
        let tmp = std::env::temp_dir().join(format!(
            "mty-rag-in-memory-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let store = VectorStore::local(&tmp);
        Self {
            chunker: Chunker::default(),
            store,
            pending: Vec::new(),
            built_doc_ids: Vec::new(),
        }
    }

    /// Override the chunker (strategy / cap / overlap / counter).
    #[must_use]
    pub fn with_chunker(mut self, chunker: Chunker) -> Self {
        self.chunker = chunker;
        self
    }

    /// Shortcut: pick a strategy without building a full [`Chunker`].
    #[must_use]
    pub fn with_strategy(mut self, strategy: ChunkStrategy) -> Self {
        self.chunker = Chunker::new(strategy);
        self
    }

    /// Override the embedder. Same surface as
    /// [`VectorStore::with_embedder`].
    #[must_use]
    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.store = std::mem::replace(&mut self.store, VectorStore::local(""))
            .with_embedder(embedder);
        self
    }

    /// Stage a `Doc` for the next `build()`. Returns `&mut Self` so
    /// callers can chain `add_text(...).add_text(...).build()?`.
    pub fn add_doc(&mut self, doc: Doc) -> &mut Self {
        self.pending.push(doc);
        self
    }

    /// Stage a text body with an auto-generated id and optional
    /// metadata. The id is derived from the staging order
    /// (`"text-{n}"`) which keeps stable + collision-free across the
    /// staged batch.
    pub fn add_text(
        &mut self,
        text: impl Into<String>,
        metadata: HashMap<String, Value>,
    ) -> &mut Self {
        let id = format!("text-{}", self.pending.len() + self.built_doc_ids.len());
        let doc = Doc {
            id,
            text: text.into(),
            metadata,
        };
        self.add_doc(doc)
    }

    /// Stage a file from disk. Equivalent to `add_doc(Doc::from_file(p)?)`.
    pub fn add_file(&mut self, path: impl AsRef<Path>) -> Result<&mut Self, IndexErr> {
        let doc = Doc::from_file(path)?;
        Ok(self.add_doc(doc))
    }

    /// Drain pending docs, chunk + embed + upsert each into the store.
    /// Returns the number of chunks indexed.
    pub fn build(&mut self) -> Result<usize, IndexErr> {
        let pending = std::mem::take(&mut self.pending);
        let mut total_chunks = 0;
        for doc in pending {
            // Purge prior chunks for the same id so re-adding a doc
            // replaces (not duplicates).
            if self.built_doc_ids.contains(&doc.id) {
                self.purge_doc_chunks(&doc.id)?;
            } else {
                self.built_doc_ids.push(doc.id.clone());
            }
            let chunks = self.chunker.chunk(&doc);
            for chunk in chunks {
                self.store
                    .upsert(&chunk.id, &chunk.text, chunk.metadata)?;
                total_chunks += 1;
            }
        }
        Ok(total_chunks)
    }

    /// Number of chunks currently in the store. After `build()`, this
    /// is the sum of chunks across every built doc.
    pub fn chunk_count(&self) -> usize {
        self.store.len()
    }

    /// Number of distinct docs built into the store (not counting
    /// pending).
    pub fn doc_count(&self) -> usize {
        self.built_doc_ids.len()
    }

    /// Number of docs staged but not yet built.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Drop every staged + built doc, clear the store.
    pub fn clear(&mut self) -> Result<(), IndexErr> {
        self.pending.clear();
        self.built_doc_ids.clear();
        self.store.clear()?;
        Ok(())
    }

    /// kNN search over the built index.
    pub fn search(&self, query: &str, k: usize) -> Result<Vec<Hit>, IndexErr> {
        Ok(self.store.search(query, k)?)
    }

    /// Borrow the underlying [`VectorStore`].
    pub fn store(&self) -> &VectorStore {
        &self.store
    }

    fn purge_doc_chunks(&mut self, doc_id: &str) -> Result<(), IndexErr> {
        // Best-effort: iterate by chunk-idx until the store says
        // not-found. Cheap when ≤ a few hundred chunks per doc.
        let mut idx = 0;
        loop {
            let id = format!("{doc_id}::chunk-{idx}");
            match self.store.delete(&id) {
                Ok(()) => idx += 1,
                Err(VectorErr::NotFound(_)) => break,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rag::chunking::ChunkStrategy;
    use tempfile::tempdir;

    #[test]
    fn build_indexes_all_pending_chunks() {
        let dir = tempdir().unwrap();
        let mut idx = Index::new(dir.path()).with_strategy(ChunkStrategy::ByParagraph);
        idx.add_text("alpha beta", HashMap::new())
            .add_text("gamma delta\n\nepsilon", HashMap::new());
        let n = idx.build().unwrap();
        assert!(n >= 2);
        assert_eq!(idx.pending_count(), 0);
        assert_eq!(idx.doc_count(), 2);
    }

    #[test]
    fn search_returns_topk() {
        let dir = tempdir().unwrap();
        let mut idx = Index::new(dir.path());
        idx.add_text("anthropic claude opus", HashMap::new())
            .add_text("anthropic claude haiku", HashMap::new())
            .add_text("totally unrelated text", HashMap::new());
        idx.build().unwrap();
        let hits = idx.search("claude", 2).unwrap();
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|h| h.text.contains("claude")));
    }

    #[test]
    fn re_add_replaces_prior_chunks() {
        let dir = tempdir().unwrap();
        let mut idx = Index::new(dir.path()).with_strategy(ChunkStrategy::ByParagraph);
        idx.add_doc(Doc::new("d1", "first version of the text"));
        idx.build().unwrap();
        let before = idx.chunk_count();
        idx.add_doc(Doc::new("d1", "second version one\n\nsecond version two\n\nthird"));
        idx.build().unwrap();
        let after = idx.chunk_count();
        assert!(after >= before, "expected at least as many chunks");
        // Search should surface the *new* content, not the old.
        let hits = idx.search("version", 5).unwrap();
        assert!(hits.iter().any(|h| h.text.contains("second")));
        assert!(!hits.iter().any(|h| h.text.contains("first version")));
    }

    #[test]
    fn add_file_round_trips() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("a.md");
        std::fs::write(&f, "alpha\n\nbeta\n\ngamma").unwrap();
        let mut idx = Index::new(dir.path()).with_strategy(ChunkStrategy::ByParagraph);
        idx.add_file(&f).unwrap();
        let n = idx.build().unwrap();
        assert!(n >= 1);
    }

    #[test]
    fn clear_drops_everything() {
        let dir = tempdir().unwrap();
        let mut idx = Index::new(dir.path());
        idx.add_text("alpha", HashMap::new());
        idx.build().unwrap();
        assert!(idx.chunk_count() > 0);
        idx.clear().unwrap();
        assert_eq!(idx.chunk_count(), 0);
        assert_eq!(idx.doc_count(), 0);
        assert_eq!(idx.pending_count(), 0);
    }

    #[test]
    fn in_memory_index_works_offline() {
        let mut idx = Index::in_memory();
        idx.add_text("hello world", HashMap::new());
        idx.build().unwrap();
        let hits = idx.search("hello", 1).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn pending_count_tracks_staging() {
        let dir = tempdir().unwrap();
        let mut idx = Index::new(dir.path());
        assert_eq!(idx.pending_count(), 0);
        idx.add_text("a", HashMap::new()).add_text("b", HashMap::new());
        assert_eq!(idx.pending_count(), 2);
        idx.build().unwrap();
        assert_eq!(idx.pending_count(), 0);
    }
}
