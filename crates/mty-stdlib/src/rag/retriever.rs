//! `std.rag.Retriever` — kNN + score-threshold layer over an [`Index`].
//!
//! Separated from `Index` so callers can swap retrieval policy (top-k,
//! score threshold, MMR diversification) without rebuilding the store.

use serde::{Deserialize, Serialize};

use crate::memory::vector::Hit;

use super::index::{Index, IndexErr};

/// Tunable retriever knobs. Defaults: `top_k=5`, no score threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrieverConfig {
    /// How many hits to return at most.
    pub top_k: usize,
    /// Drop any hit with score below this. `None` = keep all.
    pub min_score: Option<f32>,
    /// When `true`, apply Maximal Marginal Relevance to diversify the
    /// top-k. v0.33 keeps the implementation simple: greedy MMR with
    /// lambda=0.5 against the lexical Jaccard similarity of the hit
    /// texts (no second embedding call). Off by default — enable when
    /// hits are too clustered.
    pub mmr: bool,
}

impl Default for RetrieverConfig {
    fn default() -> Self {
        Self {
            top_k: 5,
            min_score: None,
            mmr: false,
        }
    }
}

/// Stateless retrieval policy. Borrows an [`Index`] and runs `search`
/// with the configured top-k + filters applied.
pub struct Retriever<'a> {
    index: &'a Index,
    config: RetrieverConfig,
}

impl<'a> Retriever<'a> {
    pub fn new(index: &'a Index) -> Self {
        Self {
            index,
            config: RetrieverConfig::default(),
        }
    }

    #[must_use]
    pub fn with_top_k(mut self, k: usize) -> Self {
        self.config.top_k = k.max(1);
        self
    }

    #[must_use]
    pub fn with_min_score(mut self, s: f32) -> Self {
        self.config.min_score = Some(s);
        self
    }

    #[must_use]
    pub fn with_mmr(mut self, on: bool) -> Self {
        self.config.mmr = on;
        self
    }

    #[must_use]
    pub fn with_config(mut self, config: RetrieverConfig) -> Self {
        self.config = config;
        self
    }

    pub fn config(&self) -> &RetrieverConfig {
        &self.config
    }

    /// Retrieve hits for `query`, applying top-k + min_score + MMR.
    /// Over-fetches by 4× when MMR is on so the diversification step
    /// has room to discard near-duplicates.
    pub fn retrieve(&self, query: &str) -> Result<Vec<Hit>, IndexErr> {
        let k = self.config.top_k;
        let fetch = if self.config.mmr { k * 4 } else { k };
        let mut hits = self.index.search(query, fetch.max(1))?;
        if let Some(min) = self.config.min_score {
            hits.retain(|h| h.score >= min);
        }
        if self.config.mmr {
            hits = mmr_diversify(hits, k, 0.5);
        } else {
            hits.truncate(k);
        }
        Ok(hits)
    }
}

/// Greedy MMR. Lexical Jaccard is good enough as a similarity proxy
/// when both texts are short; no second embedding call needed.
fn mmr_diversify(hits: Vec<Hit>, k: usize, lambda: f32) -> Vec<Hit> {
    if hits.len() <= k {
        return hits;
    }
    let mut remaining = hits;
    let mut chosen: Vec<Hit> = Vec::new();
    // Take the top-scored hit unconditionally.
    if let Some(first) = remaining.first().cloned() {
        chosen.push(first);
        remaining.remove(0);
    }
    while chosen.len() < k && !remaining.is_empty() {
        let mut best_idx = 0;
        let mut best_mmr = f32::MIN;
        for (i, h) in remaining.iter().enumerate() {
            let sim_to_chosen: f32 = chosen
                .iter()
                .map(|c| jaccard(&c.text, &h.text))
                .fold(0.0_f32, f32::max);
            let mmr = lambda * h.score - (1.0 - lambda) * sim_to_chosen;
            if mmr > best_mmr {
                best_mmr = mmr;
                best_idx = i;
            }
        }
        chosen.push(remaining.remove(best_idx));
    }
    chosen
}

fn jaccard(a: &str, b: &str) -> f32 {
    use std::collections::HashSet;
    let ta: HashSet<&str> = a.split_whitespace().collect();
    let tb: HashSet<&str> = b.split_whitespace().collect();
    if ta.is_empty() && tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count() as f32;
    let union = ta.union(&tb).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::tempdir;

    fn build_idx() -> Index {
        let dir = tempdir().unwrap();
        let mut idx = Index::new(dir.path().join("r"));
        idx.add_text("anthropic claude opus model", HashMap::new())
            .add_text("anthropic claude haiku model", HashMap::new())
            .add_text("openai gpt model", HashMap::new())
            .add_text("totally unrelated kitchen recipe", HashMap::new());
        idx.build().unwrap();
        idx
    }

    #[test]
    fn retrieve_respects_top_k() {
        let idx = build_idx();
        let r = Retriever::new(&idx).with_top_k(2);
        let hits = r.retrieve("claude model").unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn min_score_drops_low_relevance_hits() {
        let idx = build_idx();
        // A min_score above what unrelated text can hit should leave
        // only the directly-on-topic results.
        let r = Retriever::new(&idx).with_top_k(10).with_min_score(0.3);
        let hits = r.retrieve("claude").unwrap();
        assert!(hits.iter().all(|h| h.score >= 0.3));
        assert!(!hits.iter().any(|h| h.text.contains("kitchen")));
    }

    #[test]
    fn mmr_returns_topk_size() {
        let idx = build_idx();
        let r = Retriever::new(&idx).with_top_k(2).with_mmr(true);
        let hits = r.retrieve("claude model").unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn default_top_k_is_five() {
        let idx = build_idx();
        let r = Retriever::new(&idx);
        let hits = r.retrieve("claude").unwrap();
        assert!(hits.len() <= 5);
    }

    #[test]
    fn config_builder_round_trips() {
        let idx = build_idx();
        let cfg = RetrieverConfig {
            top_k: 3,
            min_score: Some(0.1),
            mmr: true,
        };
        let r = Retriever::new(&idx).with_config(cfg);
        assert_eq!(r.config().top_k, 3);
        assert_eq!(r.config().min_score, Some(0.1));
        assert!(r.config().mmr);
    }
}
