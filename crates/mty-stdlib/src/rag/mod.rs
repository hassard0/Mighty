//! `std.rag` — Retrieval-Augmented Generation as a stdlib surface.
//!
//! v0.33 Track T2 promotes the v0.26 Track C `std.memory.VectorStore`
//! into a one-liner RAG pipeline:
//!
//! ```ignore
//! use std.rag.{Index, Doc, Rag};
//! use std.swarm.Member;
//!
//! let mut index = Index.new("./corpus")
//! index.add_text("Mighty is a typed agent-first language", {source: "intro"})
//! index.add_file("./docs/spec.md")?
//! index.build()?
//!
//! let rag = Rag.new()
//!   .with_index(index)
//!   .with_retriever_top_k(5)
//!   .with_member(Member.anthropic("claude-opus-4-7"))
//!
//! let answer = rag.ask("What's Mighty's capability typing?")?
//! ```
//!
//! ## Module layout
//!
//! - [`doc`] — [`Doc`] source unit + helpers
//! - [`chunking`] — 4 text-splitting strategies
//! - [`index`] — [`Index`] wraps a `VectorStore` + chunker
//! - [`retriever`] — kNN search + score thresholding
//! - [`reranker`] — optional LLM-as-reranker
//! - [`pipeline`] — [`Rag`] end-to-end glue
//!
//! ## Design notes
//!
//! `Index` owns a [`VectorStore`](crate::memory::VectorStore) and a
//! [`Chunker`](chunking::Chunker). `add_text` / `add_file` accumulate
//! `Doc`s into a staging buffer; `build()` runs the chunker over every
//! pending doc, embeds each chunk, and upserts into the underlying
//! store. The two-phase add/build split lets callers batch I/O without
//! firing off thousands of single-chunk upserts.
//!
//! The default chunker is paragraph-based with a 1024-token soft cap
//! (matches typical retrieval-augmented-generation literature for
//! short-form docs). Callers who know their corpus (code, prose,
//! markdown) plug in [`chunking::ChunkStrategy::ByTokens`],
//! [`chunking::ChunkStrategy::BySection`], or
//! [`chunking::ChunkStrategy::ByCodeFence`].

pub mod chunking;
pub mod doc;
pub mod index;
pub mod pipeline;
pub mod reranker;
pub mod retriever;

pub use chunking::{Chunk, ChunkStrategy, Chunker};
pub use doc::Doc;
pub use index::{Index, IndexErr};
pub use pipeline::{Rag, RagErr};
pub use reranker::Reranker;
pub use retriever::{Retriever, RetrieverConfig};
