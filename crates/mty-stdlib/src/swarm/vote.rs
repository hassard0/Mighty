//! Answer comparison + clustering.
//!
//! Two distinct cases dominate real swarms:
//!
//! - **Yes/no (or short-answer) prompts.** "Is this code safe?" →
//!   `yes` / `no` / `i don't know`. Cheap to compare: trim, lower-case,
//!   strip punctuation, exact-match.
//! - **Free-form prompts.** "Summarise the safety risks." → three
//!   paragraphs that *say the same thing in different words*.
//!   Levenshtein over the whole body is too noisy; we use a
//!   normalised-token-set Jaccard (cheap, deterministic, no embed
//!   model required) and cluster by a tunable similarity floor.
//!
//! The [`cluster_replies`] function takes a slice of bodies and emits
//! a `Vec<Cluster>` where each `Cluster.indices` is the original
//! reply indices grouped under one representative answer. The
//! consensus strategies in [`crate::swarm::consensus`] pick a cluster
//! based on their own rules (majority, weighted, unanimous, first).
//!
//! ## Why no embedding-cosine path today
//!
//! `std.llm.embed` doesn't ship until v0.28. When it lands, the
//! [`SimilarityMode::Embedding`] variant will route through
//! `LlmProvider::embed` to score body-pair cosines. Until then,
//! callers stick with the token-set Jaccard default; for prompts
//! where that's too lossy, the future LLM-judge mode (also v0.28)
//! lets Claude evaluate "do these N say the same thing".

/// How [`cluster_replies`] decides whether two bodies are "the same".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SimilarityMode {
    /// Trim, lower-case, strip punctuation, exact-match. Cheap.
    /// Ideal for yes/no answers.
    Exact,
    /// Normalised-token-set Jaccard. Two bodies cluster together if
    /// their Jaccard score >= the threshold passed to
    /// [`cluster_replies`]. Default threshold is `0.6`.
    #[default]
    TokenSet,
}

/// One cluster of replies that the [`SimilarityMode`] judged
/// equivalent. `representative` is the first body that landed in
/// the cluster; consensus surfaces use it as the cluster's canonical
/// answer.
#[derive(Debug, Clone)]
pub struct Cluster {
    /// Indices (into the original replies slice) that fell into this
    /// cluster.
    pub indices: Vec<usize>,
    /// Canonical body — the first one to land in the cluster.
    pub representative: String,
}

impl Cluster {
    pub fn size(&self) -> usize {
        self.indices.len()
    }
}

/// Cluster `bodies` by similarity. Returns clusters in *insertion
/// order*: the first body becomes cluster 0; each subsequent body
/// either joins an existing cluster (if it scores above `threshold`
/// against the representative) or starts a new one.
///
/// Stable sort by `size` happens in the consensus layer (callers that
/// want "biggest cluster first" sort after).
pub fn cluster_replies(bodies: &[String], mode: SimilarityMode, threshold: f32) -> Vec<Cluster> {
    let mut clusters: Vec<Cluster> = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        let mut joined = false;
        for c in clusters.iter_mut() {
            if similarity(body, &c.representative, mode) >= threshold {
                c.indices.push(i);
                joined = true;
                break;
            }
        }
        if !joined {
            clusters.push(Cluster {
                indices: vec![i],
                representative: body.clone(),
            });
        }
    }
    clusters
}

/// Pairwise similarity in `[0.0, 1.0]`. `1.0` = identical under the
/// chosen normalisation; `0.0` = no overlap.
pub fn similarity(a: &str, b: &str, mode: SimilarityMode) -> f32 {
    match mode {
        SimilarityMode::Exact => {
            if normalise_short(a) == normalise_short(b) {
                1.0
            } else {
                0.0
            }
        }
        SimilarityMode::TokenSet => jaccard_token_set(a, b),
    }
}

/// "yes" / "Yes!" / " YES.\n" all collapse to "yes".
fn normalise_short(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .trim()
        .to_lowercase()
}

/// Tokenise by whitespace, drop punctuation, lower-case, then Jaccard
/// over the resulting sets.
fn jaccard_token_set(a: &str, b: &str) -> f32 {
    let ta = tokens(a);
    let tb = tokens(b);
    if ta.is_empty() && tb.is_empty() {
        return 1.0;
    }
    let mut intersect = 0u32;
    for t in &ta {
        if tb.contains(t) {
            intersect += 1;
        }
    }
    let union = (ta.len() + tb.len()) as u32 - intersect;
    if union == 0 {
        return 1.0;
    }
    intersect as f32 / union as f32
}

fn tokens(s: &str) -> std::collections::BTreeSet<String> {
    // Split on whitespace AND non-alphanumeric punctuation so
    // "well-tested" → ["well", "tested"] rather than "welltested".
    // That keeps paraphrases like "well tested" + "well-tested" in
    // the same cluster.
    s.split(|c: char| c.is_whitespace() || (!c.is_alphanumeric()))
        .map(|t| t.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_mode_normalises_short_yes() {
        assert_eq!(similarity("yes", "  YES!\n", SimilarityMode::Exact), 1.0);
        assert_eq!(similarity("yes", "no", SimilarityMode::Exact), 0.0);
    }

    #[test]
    fn token_set_mode_clusters_paraphrases() {
        let a = "The code looks safe and well-tested.".to_string();
        let b = "code looks well tested and safe".to_string();
        let s = similarity(&a, &b, SimilarityMode::TokenSet);
        assert!(s > 0.6, "expected high jaccard, got {s}");
    }

    #[test]
    fn cluster_replies_groups_by_threshold() {
        let bodies = vec![
            "yes safe".to_string(),
            "yes safe".to_string(),
            "no, do not deploy".to_string(),
        ];
        let cs = cluster_replies(&bodies, SimilarityMode::TokenSet, 0.6);
        assert_eq!(cs.len(), 2);
        // First cluster holds the two "yes safe" replies.
        let big = cs.iter().max_by_key(|c| c.size()).unwrap();
        assert_eq!(big.size(), 2);
    }

    #[test]
    fn cluster_replies_singleton_per_member_when_all_disagree() {
        let bodies = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        let cs = cluster_replies(&bodies, SimilarityMode::TokenSet, 0.6);
        assert_eq!(cs.len(), 4);
        for c in &cs {
            assert_eq!(c.size(), 1);
        }
    }

    #[test]
    fn empty_inputs_cluster_to_zero() {
        let cs = cluster_replies(&[], SimilarityMode::TokenSet, 0.6);
        assert!(cs.is_empty());
    }
}
