//! `std.memory` — embeddings.
//!
//! The [`Embedder`] trait abstracts over "give me a fixed-width float
//! vector for this text". Two implementations ship in v0.26:
//!
//! 1. [`StubEmbedder`] — deterministic hash-based embedder. Cheap,
//!    offline, and bit-stable across runs. The default for tests + for
//!    local builds where no API key is configured. **Not semantic** —
//!    cosine similarity here is essentially "did these strings share
//!    tokens", but it's stable enough for unit tests + replay.
//!
//! 2. [`OpenAIEmbedder`] — wraps Track A's `std.llm` provider trait to
//!    request `text-embedding-3-small` embeddings. Gated behind the
//!    `memory-openai` feature; when the feature is off, the type is
//!    still defined so call-sites compile but `embed` returns
//!    `EmbeddingErr::FeatureDisabled`.
//!
//! Vector stores accept any `Embedder` by trait, so a downstream
//! caller can plug in a third-party embedder (e.g. local sentence-
//! transformers via FFI) without touching the rest of the module.

use std::sync::Arc;

/// Errors returned by an [`Embedder`].
#[derive(Debug, thiserror::Error)]
pub enum EmbeddingErr {
    /// The configured backend is missing a feature flag at compile time.
    #[error("embedding backend `{0}` is disabled in this build (feature flag off)")]
    FeatureDisabled(&'static str),
    /// Network / provider error from the underlying client.
    #[error("embedding provider error: {0}")]
    Provider(String),
    /// The provider returned a malformed response.
    #[error("embedding decode: {0}")]
    Decode(String),
}

/// Dimensionality of the [`StubEmbedder`] output. Picked small enough
/// to be cheap in tests but large enough to discriminate ~hundreds of
/// distinct documents in nearest-neighbour search.
pub const STUB_EMBED_DIM: usize = 64;

/// "Give me a vector for this text." The vector dimensionality is
/// backend-defined; the [`VectorStore`](super::vector::VectorStore)
/// caches the first vector's `len()` it sees and enforces all
/// subsequent vectors match.
pub trait Embedder: Send + Sync {
    /// Stable backend name. Used by snapshot/restore to detect
    /// embedder mismatches when a store is restored against a
    /// different backend.
    fn name(&self) -> &'static str;

    /// Vector dimensionality. Constant per embedder.
    fn dim(&self) -> usize;

    /// Encode `text` into a vector. Implementations should be
    /// deterministic when possible (the [`StubEmbedder`] is; OpenAI
    /// isn't, but its outputs are stable per text + model version).
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingErr>;

    /// Batch convenience — default is `text.iter().map(self.embed)`.
    /// Backends with native batch APIs (OpenAI) override for the
    /// roundtrip win.
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingErr> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

// -----------------------------------------------------------------------------
// StubEmbedder — deterministic, offline, default.
// -----------------------------------------------------------------------------

/// Deterministic hash-based embedder. The output vector is built by
/// folding each whitespace-delimited token's FNV-1a hash into the
/// configured-width float vector, then L2-normalising. Cheap and
/// fully repeatable — bit-identical across platforms.
///
/// Cosine similarity on these vectors approximates token-overlap
/// scoring: two strings sharing many tokens will land close, two
/// strings with no overlap will land near-orthogonal. That's plenty
/// for tests + the local "I'm offline" path.
#[derive(Debug, Clone, Copy, Default)]
pub struct StubEmbedder {
    dim: usize,
}

impl StubEmbedder {
    /// Build a stub embedder with the default [`STUB_EMBED_DIM`]
    /// dimensionality.
    pub fn new() -> Self {
        Self {
            dim: STUB_EMBED_DIM,
        }
    }

    /// Build a stub embedder with an explicit dimensionality. Useful
    /// when callers want to stress-test high-dim cosine math without
    /// pulling in a real model.
    pub fn with_dim(dim: usize) -> Self {
        Self { dim: dim.max(1) }
    }
}

impl Embedder for StubEmbedder {
    fn name(&self) -> &'static str {
        "stub-fnv"
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingErr> {
        let mut v = vec![0.0f32; self.dim];
        // Lower-case + split on whitespace + punctuation so "Hello!"
        // and "hello" align.
        let normalised: String = text
            .chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    ' '
                }
            })
            .collect();
        for token in normalised.split_whitespace() {
            if token.is_empty() {
                continue;
            }
            let h = fnv1a64(token.as_bytes());
            // Fold into the vector — bucket = h % dim, contribution
            // sign = next bit of the hash. Magnitude is constant so
            // every token contributes equally before normalisation.
            let bucket = (h as usize) % self.dim;
            let sign = if (h >> 32) & 1 == 0 { 1.0 } else { -1.0 };
            v[bucket] += sign;
        }
        l2_normalise(&mut v);
        Ok(v)
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

fn l2_normalise(v: &mut [f32]) {
    let mag: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag > 0.0 {
        for x in v.iter_mut() {
            *x /= mag;
        }
    }
}

// -----------------------------------------------------------------------------
// OpenAIEmbedder — opt-in via the `memory-openai` feature.
// -----------------------------------------------------------------------------

/// OpenAI `text-embedding-3-small` embedder. The full HTTP wiring is
/// gated behind the `memory-openai` feature; without it the type is
/// kept for API symmetry but [`embed`](Embedder::embed) returns
/// [`EmbeddingErr::FeatureDisabled`].
///
/// When Track A's `std.llm` provider trait lands, the real impl
/// should delegate to the provider's embedding endpoint instead of
/// re-implementing HTTP — see `dev/history/notes/STD_MEMORY_V0_26_NOTES.md`.
#[derive(Debug, Clone)]
pub struct OpenAIEmbedder {
    // Held for the live-HTTP path (`memory-openai` feature) and for
    // snapshot diagnostics; the default build never reads them but we
    // don't want the values to silently drop.
    #[allow(dead_code)]
    api_key: String,
    #[allow(dead_code)]
    model: String,
    dim: usize,
}

impl OpenAIEmbedder {
    /// Build an OpenAI embedder. `model` defaults to
    /// `text-embedding-3-small` (1536 dims) when blank.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        let model = model.into();
        let dim = if model.contains("3-large") {
            3072
        } else {
            1536
        };
        Self {
            api_key: api_key.into(),
            model: if model.is_empty() {
                "text-embedding-3-small".to_string()
            } else {
                model
            },
            dim,
        }
    }
}

impl Embedder for OpenAIEmbedder {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn dim(&self) -> usize {
        self.dim
    }

    #[allow(unused_variables)]
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingErr> {
        #[cfg(feature = "memory-openai")]
        {
            // The Track A `std.llm` integration is the right home for
            // the actual HTTP call. v0.26 ships this as a placeholder
            // so downstream code can compile against the trait shape
            // even when the feature is off; a real implementation will
            // delegate to `crate::llm::openai::embed(...)` once Track A
            // exposes it.
            let _ = (&self.api_key, &self.model);
            Err(EmbeddingErr::Provider(
                "openai embeddings: live HTTP not yet wired — pending Track A llm::openai::embed"
                    .into(),
            ))
        }
        #[cfg(not(feature = "memory-openai"))]
        {
            let _ = text;
            Err(EmbeddingErr::FeatureDisabled("memory-openai"))
        }
    }
}

/// Construct the default embedder for the current build. Picks the
/// stub embedder unconditionally — production code should choose
/// explicitly via [`VectorStore::with_embedder`](super::vector::VectorStore::with_embedder).
pub fn default_embedder() -> Arc<dyn Embedder> {
    Arc::new(StubEmbedder::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_dim_matches_constant() {
        let e = StubEmbedder::new();
        let v = e.embed("hello world").unwrap();
        assert_eq!(v.len(), STUB_EMBED_DIM);
        assert_eq!(e.dim(), STUB_EMBED_DIM);
    }

    #[test]
    fn stub_with_dim_respects_dim() {
        let e = StubEmbedder::with_dim(8);
        assert_eq!(e.embed("hi").unwrap().len(), 8);
    }

    #[test]
    fn stub_with_dim_clamps_to_minimum_one() {
        let e = StubEmbedder::with_dim(0);
        assert_eq!(e.dim(), 1);
        assert_eq!(e.embed("x").unwrap().len(), 1);
    }

    #[test]
    fn stub_is_deterministic() {
        let e = StubEmbedder::new();
        let a = e.embed("the quick brown fox").unwrap();
        let b = e.embed("the quick brown fox").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn stub_is_l2_normalised() {
        let e = StubEmbedder::new();
        let v = e.embed("anthropic claude opus").unwrap();
        let mag: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((mag - 1.0).abs() < 1e-5, "magnitude = {mag}");
    }

    #[test]
    fn stub_empty_string_yields_zero_vector() {
        let e = StubEmbedder::new();
        let v = e.embed("").unwrap();
        assert_eq!(v.len(), STUB_EMBED_DIM);
        assert!(v.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn stub_token_overlap_increases_similarity() {
        let e = StubEmbedder::new();
        let a = e.embed("anthropic claude opus").unwrap();
        let b = e.embed("anthropic claude haiku").unwrap();
        let c = e.embed("totally unrelated text").unwrap();
        let sim_ab: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
        let sim_ac: f32 = a.iter().zip(&c).map(|(x, y)| x * y).sum();
        assert!(sim_ab > sim_ac, "ab={sim_ab} ac={sim_ac}");
    }

    #[test]
    fn embed_batch_default_matches_per_item() {
        let e = StubEmbedder::new();
        let texts = ["one", "two", "three"];
        let batch = e.embed_batch(&texts).unwrap();
        for (i, t) in texts.iter().enumerate() {
            assert_eq!(batch[i], e.embed(t).unwrap());
        }
    }

    #[test]
    fn openai_disabled_returns_feature_disabled_when_off() {
        let e = OpenAIEmbedder::new("sk-xxx", "");
        // Without the `memory-openai` feature, embed should fail with
        // FeatureDisabled. With the feature on, it should fail with
        // Provider (the real HTTP wiring is pending Track A).
        match e.embed("hi") {
            Err(EmbeddingErr::FeatureDisabled(_) | EmbeddingErr::Provider(_)) => {}
            other => panic!("expected disabled/provider err, got {other:?}"),
        }
    }

    #[test]
    fn openai_model_overrides_dim() {
        let small = OpenAIEmbedder::new("sk", "text-embedding-3-small");
        let large = OpenAIEmbedder::new("sk", "text-embedding-3-large");
        assert_eq!(small.dim(), 1536);
        assert_eq!(large.dim(), 3072);
    }

    #[test]
    fn default_embedder_yields_stub() {
        let e = default_embedder();
        assert_eq!(e.name(), "stub-fnv");
        assert_eq!(e.dim(), STUB_EMBED_DIM);
    }
}
