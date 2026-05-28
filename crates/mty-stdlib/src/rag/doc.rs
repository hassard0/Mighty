//! `std.rag.Doc` — a single source unit going into an [`Index`](super::Index).
//!
//! A `Doc` is a thin envelope over `(id, text, metadata)`. The id is
//! stable so re-indexing the same source replaces (not duplicates) the
//! prior chunks; the metadata travels through to every chunk so search
//! hits can attribute their text back to its origin.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// One source unit for RAG indexing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Doc {
    /// Stable id within the index. Chunks of this doc become
    /// `{id}::chunk-{n}` in the underlying vector store; re-adding a
    /// doc with the same id replaces every prior chunk.
    pub id: String,
    /// Raw text body. The chunker splits this into [`Chunk`](super::Chunk)s
    /// at [`Index::build`](super::Index::build) time.
    pub text: String,
    /// Free-form metadata carried through to every chunk + every search
    /// hit. Conventional keys: `source` (file path or URL), `title`,
    /// `section`, `language`. Stored verbatim — the retriever doesn't
    /// interpret the schema.
    #[serde(default)]
    pub metadata: HashMap<String, Value>,
}

impl Doc {
    /// Build a doc with a stable id + text body. Metadata is empty.
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            metadata: HashMap::new(),
        }
    }

    /// Builder: attach one metadata key/value.
    #[must_use]
    pub fn with_meta(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Builder: replace the metadata map wholesale.
    #[must_use]
    pub fn with_metadata(mut self, metadata: HashMap<String, Value>) -> Self {
        self.metadata = metadata;
        self
    }

    /// Read a file from disk and build a [`Doc`] from it. The id
    /// defaults to the file's path string; the `source` metadata
    /// records the same. Returns an `io::Error` on read failure.
    pub fn from_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let p = path.as_ref();
        let text = std::fs::read_to_string(p)?;
        let id = p.display().to_string();
        let mut meta = HashMap::new();
        meta.insert("source".into(), Value::String(id.clone()));
        Ok(Self {
            id,
            text,
            metadata: meta,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn new_doc_has_empty_metadata() {
        let d = Doc::new("a", "hello");
        assert_eq!(d.id, "a");
        assert_eq!(d.text, "hello");
        assert!(d.metadata.is_empty());
    }

    #[test]
    fn with_meta_round_trips() {
        let d = Doc::new("a", "x").with_meta("source", Value::String("docs/x.md".into()));
        assert_eq!(
            d.metadata.get("source"),
            Some(&Value::String("docs/x.md".into()))
        );
    }

    #[test]
    fn from_file_reads_text_and_attaches_source() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("a.md");
        std::fs::write(&p, "# heading\n\nbody").unwrap();
        let d = Doc::from_file(&p).unwrap();
        assert!(d.text.contains("body"));
        assert_eq!(
            d.metadata.get("source"),
            Some(&Value::String(p.display().to_string()))
        );
    }
}
