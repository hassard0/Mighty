//! `std.rag.chunking` — text-splitting strategies for [`Doc`](super::Doc)s.
//!
//! Four strategies ship in v0.33:
//!
//! 1. [`ChunkStrategy::ByTokens`] — fixed-width approximate-token
//!    windows. Default span 1024 tokens with 64-token overlap. Good
//!    catch-all when you don't know the corpus shape.
//! 2. [`ChunkStrategy::ByParagraph`] — split on blank lines, then
//!    merge adjacent paragraphs up to the soft token cap. Best for
//!    natural prose. **Default for [`Chunker::default`].**
//! 3. [`ChunkStrategy::BySection`] — split on Markdown headings
//!    (`#`, `##`, `###`). Best for technical docs / wikis where each
//!    section is a self-contained unit.
//! 4. [`ChunkStrategy::ByCodeFence`] — split on triple-backtick fences.
//!    Best for code-heavy docs (tutorials, examples) where keeping a
//!    code block intact matters more than the soft token cap.
//!
//! "Tokens" here means whitespace-delimited words — an approximation
//! good to ~25% of a real BPE tokenizer's count but with no
//! tokenizer-data-file dependency. Callers who want exact token counts
//! supply their own [`Chunker::with_token_counter`].

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use super::doc::Doc;

/// One output of [`Chunker::chunk`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Chunk {
    /// Stable id derived from the source doc id + chunk index:
    /// `"{doc_id}::chunk-{n}"`. Re-chunking the same doc produces the
    /// same ids in the same order.
    pub id: String,
    /// The chunk's text body.
    pub text: String,
    /// Source doc id this chunk came from. Always set.
    pub doc_id: String,
    /// Chunk index within the doc (0-based).
    pub chunk_idx: usize,
    /// Per-chunk metadata. Starts as a clone of the source doc's
    /// metadata plus `chunk_idx` for retrieval-side filtering.
    pub metadata: HashMap<String, Value>,
}

/// Which strategy [`Chunker`] uses to split a doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkStrategy {
    /// Fixed approximate-token windows. See module docs.
    ByTokens,
    /// Split on blank lines, merge under the soft cap. **Default.**
    ByParagraph,
    /// Split on Markdown `#`, `##`, `###` headings.
    BySection,
    /// Split on triple-backtick fences (`” ``` ”`).
    ByCodeFence,
}

impl Default for ChunkStrategy {
    fn default() -> Self {
        Self::ByParagraph
    }
}

/// Pluggable token counter — given a string, return an approximate
/// token count. Default implementation counts whitespace-delimited
/// words.
pub type TokenCounter = Arc<dyn Fn(&str) -> usize + Send + Sync>;

fn default_token_counter() -> TokenCounter {
    Arc::new(|s: &str| s.split_whitespace().count())
}

/// Splits [`Doc`]s into [`Chunk`]s under a [`ChunkStrategy`].
#[derive(Clone)]
pub struct Chunker {
    strategy: ChunkStrategy,
    /// Soft cap. Strategies merge / split to stay under this limit; the
    /// cap can be exceeded for atomic units (e.g. a single oversized
    /// paragraph or code fence) — the alternative is silently truncating
    /// the doc, which is worse for retrieval.
    max_tokens: usize,
    /// Overlap between consecutive chunks for `ByTokens`. Ignored by
    /// the other strategies (paragraph / section / code-fence boundaries
    /// already give natural overlap).
    overlap_tokens: usize,
    counter: TokenCounter,
}

impl std::fmt::Debug for Chunker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chunker")
            .field("strategy", &self.strategy)
            .field("max_tokens", &self.max_tokens)
            .field("overlap_tokens", &self.overlap_tokens)
            .finish()
    }
}

impl Default for Chunker {
    fn default() -> Self {
        Self {
            strategy: ChunkStrategy::default(),
            max_tokens: 1024,
            overlap_tokens: 64,
            counter: default_token_counter(),
        }
    }
}

impl Chunker {
    /// Build with the given strategy + the default 1024-token cap.
    pub fn new(strategy: ChunkStrategy) -> Self {
        Self {
            strategy,
            ..Self::default()
        }
    }

    /// Override the soft token cap. Strategies merge / split to stay
    /// under it; atomic units (one paragraph, one code fence) may
    /// exceed.
    #[must_use]
    pub fn with_max_tokens(mut self, n: usize) -> Self {
        self.max_tokens = n.max(1);
        self
    }

    /// Override the per-chunk overlap (only used by [`ChunkStrategy::ByTokens`]).
    #[must_use]
    pub fn with_overlap_tokens(mut self, n: usize) -> Self {
        self.overlap_tokens = n;
        self
    }

    /// Plug in a custom token counter (e.g. a real BPE tokenizer FFI).
    #[must_use]
    pub fn with_token_counter(mut self, counter: TokenCounter) -> Self {
        self.counter = counter;
        self
    }

    /// Current strategy.
    pub fn strategy(&self) -> ChunkStrategy {
        self.strategy
    }

    /// Current soft cap.
    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    /// Run the configured strategy over `doc`.
    pub fn chunk(&self, doc: &Doc) -> Vec<Chunk> {
        let raw_chunks: Vec<String> = match self.strategy {
            ChunkStrategy::ByTokens => self.chunk_by_tokens(&doc.text),
            ChunkStrategy::ByParagraph => self.chunk_by_paragraph(&doc.text),
            ChunkStrategy::BySection => self.chunk_by_section(&doc.text),
            ChunkStrategy::ByCodeFence => self.chunk_by_code_fence(&doc.text),
        };
        raw_chunks
            .into_iter()
            .enumerate()
            .map(|(idx, text)| {
                let mut metadata = doc.metadata.clone();
                metadata.insert(
                    "chunk_idx".into(),
                    Value::Number(serde_json::Number::from(idx)),
                );
                Chunk {
                    id: format!("{}::chunk-{idx}", doc.id),
                    text,
                    doc_id: doc.id.clone(),
                    chunk_idx: idx,
                    metadata,
                }
            })
            .collect()
    }

    fn count(&self, s: &str) -> usize {
        (self.counter)(s)
    }

    fn chunk_by_tokens(&self, text: &str) -> Vec<String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            return Vec::new();
        }
        let span = self.max_tokens.max(1);
        let overlap = self.overlap_tokens.min(span.saturating_sub(1));
        let step = (span - overlap).max(1);
        let mut out = Vec::new();
        let mut start = 0;
        while start < words.len() {
            let end = (start + span).min(words.len());
            out.push(words[start..end].join(" "));
            if end == words.len() {
                break;
            }
            start += step;
        }
        out
    }

    fn chunk_by_paragraph(&self, text: &str) -> Vec<String> {
        // Split on blank lines. A "blank line" is a line containing
        // only whitespace; collapses runs of blank lines.
        let mut paragraphs: Vec<String> = Vec::new();
        let mut buf = String::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                if !buf.is_empty() {
                    paragraphs.push(std::mem::take(&mut buf));
                }
            } else {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(line);
            }
        }
        if !buf.is_empty() {
            paragraphs.push(buf);
        }
        self.merge_under_cap(paragraphs, "\n\n")
    }

    fn chunk_by_section(&self, text: &str) -> Vec<String> {
        // Split on Markdown headings — every line starting with one or
        // more `#` characters followed by a space opens a new section.
        // Heading line is kept with the section body that follows.
        let mut sections: Vec<String> = Vec::new();
        let mut buf = String::new();
        for line in text.lines() {
            let trimmed = line.trim_start();
            let is_heading = trimmed.starts_with('#')
                && trimmed
                    .chars()
                    .skip_while(|c| *c == '#')
                    .next()
                    .map(|c| c == ' ')
                    .unwrap_or(false);
            if is_heading && !buf.is_empty() {
                sections.push(std::mem::take(&mut buf));
            }
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(line);
        }
        if !buf.is_empty() {
            sections.push(buf);
        }
        self.merge_under_cap(sections, "\n\n")
    }

    fn chunk_by_code_fence(&self, text: &str) -> Vec<String> {
        // Split on triple-backtick fences. Every fenced block stays
        // intact; prose between fences becomes its own chunk. Whitespace
        // separation between fences and prose is preserved.
        let mut chunks: Vec<String> = Vec::new();
        let mut buf = String::new();
        let mut in_fence = false;
        for line in text.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") {
                if !in_fence {
                    // Opening fence: flush prose first.
                    if !buf.is_empty() {
                        chunks.push(std::mem::take(&mut buf));
                    }
                    buf.push_str(line);
                    in_fence = true;
                } else {
                    // Closing fence: emit the fenced block as its own chunk.
                    buf.push('\n');
                    buf.push_str(line);
                    chunks.push(std::mem::take(&mut buf));
                    in_fence = false;
                }
            } else {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(line);
            }
        }
        if !buf.is_empty() {
            chunks.push(buf);
        }
        // Code fences are atomic — don't merge across them.
        chunks
            .into_iter()
            .filter(|c| !c.trim().is_empty())
            .collect()
    }

    /// Merge adjacent atomic units (paragraphs / sections) under the
    /// soft cap. An atomic unit larger than the cap on its own is
    /// emitted as a single chunk (rather than silently truncated).
    fn merge_under_cap(&self, units: Vec<String>, sep: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut buf = String::new();
        let mut buf_tokens = 0usize;
        for unit in units {
            let unit_tokens = self.count(&unit);
            if buf.is_empty() {
                buf = unit;
                buf_tokens = unit_tokens;
            } else if buf_tokens + unit_tokens <= self.max_tokens {
                buf.push_str(sep);
                buf.push_str(&unit);
                buf_tokens += unit_tokens;
            } else {
                out.push(std::mem::take(&mut buf));
                buf = unit;
                buf_tokens = unit_tokens;
            }
        }
        if !buf.is_empty() {
            out.push(buf);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Doc {
        Doc::new("d", text)
    }

    #[test]
    fn by_paragraph_splits_on_blank_lines() {
        let c = Chunker::new(ChunkStrategy::ByParagraph).with_max_tokens(4);
        let chunks = c.chunk(&doc("alpha beta\n\ngamma delta\n\nepsilon zeta"));
        assert!(chunks.len() >= 2, "expected >= 2 chunks, got {chunks:?}");
        for ch in &chunks {
            assert!(!ch.text.is_empty());
            assert_eq!(ch.doc_id, "d");
        }
    }

    #[test]
    fn by_paragraph_merges_under_cap() {
        // 3 tiny paragraphs, soft cap large enough to merge all.
        let c = Chunker::new(ChunkStrategy::ByParagraph).with_max_tokens(64);
        let chunks = c.chunk(&doc("one\n\ntwo\n\nthree"));
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.contains("one"));
        assert!(chunks[0].text.contains("three"));
    }

    #[test]
    fn by_tokens_windows_with_overlap() {
        let c = Chunker::new(ChunkStrategy::ByTokens)
            .with_max_tokens(4)
            .with_overlap_tokens(2);
        // 8 tokens → expect 3 windows of 4 with stride 2 = (0..4), (2..6), (4..8)
        let chunks = c.chunk(&doc("a b c d e f g h"));
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].text, "a b c d");
        assert_eq!(chunks[1].text, "c d e f");
        assert_eq!(chunks[2].text, "e f g h");
    }

    #[test]
    fn by_tokens_empty_text_emits_no_chunks() {
        let c = Chunker::new(ChunkStrategy::ByTokens);
        assert!(c.chunk(&doc("")).is_empty());
        assert!(c.chunk(&doc("   ")).is_empty());
    }

    #[test]
    fn by_section_splits_on_markdown_headings() {
        let md = "# Intro\n\nhello\n\n## Sub\n\nworld\n\n# Two\n\nbody";
        let c = Chunker::new(ChunkStrategy::BySection).with_max_tokens(2);
        let chunks = c.chunk(&doc(md));
        assert!(chunks.len() >= 2, "got {chunks:?}");
        assert!(chunks.iter().any(|c| c.text.contains("# Intro")));
        assert!(chunks.iter().any(|c| c.text.contains("# Two")));
    }

    #[test]
    fn by_code_fence_keeps_fences_intact() {
        let md = "prose before\n\n```rust\nfn x() {}\n```\n\nprose after";
        let c = Chunker::new(ChunkStrategy::ByCodeFence);
        let chunks = c.chunk(&doc(md));
        assert_eq!(chunks.len(), 3);
        assert!(chunks[0].text.contains("prose before"));
        assert!(chunks[1].text.contains("```rust"));
        assert!(chunks[1].text.ends_with("```"));
        assert!(chunks[2].text.contains("prose after"));
    }

    #[test]
    fn chunk_ids_are_stable_and_indexed() {
        let c = Chunker::new(ChunkStrategy::ByParagraph).with_max_tokens(1);
        let chunks = c.chunk(&doc("alpha\n\nbeta\n\ngamma"));
        for (i, ch) in chunks.iter().enumerate() {
            assert_eq!(ch.id, format!("d::chunk-{i}"));
            assert_eq!(ch.chunk_idx, i);
        }
    }

    #[test]
    fn metadata_propagates_to_chunks() {
        let d = Doc::new("d", "alpha\n\nbeta").with_meta("source", Value::String("docs".into()));
        let c = Chunker::new(ChunkStrategy::ByParagraph).with_max_tokens(1);
        let chunks = c.chunk(&d);
        for ch in &chunks {
            assert_eq!(ch.metadata.get("source"), Some(&Value::String("docs".into())));
            assert!(ch.metadata.contains_key("chunk_idx"));
        }
    }

    #[test]
    fn custom_token_counter_used() {
        // Counter that says every string has 100 tokens. Should force
        // one chunk per paragraph because every paragraph blows the cap.
        let c = Chunker::new(ChunkStrategy::ByParagraph)
            .with_max_tokens(10)
            .with_token_counter(Arc::new(|_| 100));
        let chunks = c.chunk(&doc("one\n\ntwo\n\nthree"));
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn default_strategy_is_paragraph() {
        let c = Chunker::default();
        assert_eq!(c.strategy(), ChunkStrategy::ByParagraph);
    }

    #[test]
    fn oversized_paragraph_emitted_intact() {
        // A single 100-token unit must NOT be silently dropped/truncated
        // when the cap is 5.
        let big = (0..100)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let c = Chunker::new(ChunkStrategy::ByParagraph).with_max_tokens(5);
        let chunks = c.chunk(&doc(&big));
        assert_eq!(chunks.len(), 1);
        assert_eq!(c.count(&chunks[0].text), 100);
    }
}
