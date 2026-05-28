//! `std.rag.Reranker` — optional LLM-as-reranker pass over retrieved hits.
//!
//! Re-scores the top-k from the retriever by asking a fast LLM (the
//! "reranker member") to score each hit's relevance to the query on a
//! 0-100 scale. The scores replace the cosine scores; ties break on the
//! original cosine score so behaviour degrades gracefully when the
//! reranker returns garbage.
//!
//! ## Why ship a reranker as part of the stdlib
//!
//! Real-world RAG pipelines almost always rerank: a cheap embedding
//! retrieves 20-100 candidates, a smarter (but slower) cross-encoder /
//! LLM picks the actual top-k. Without an in-the-box reranker the
//! "first real RAG project" is uneven — agents trip on the cosine-
//! similarity-isn't-relevance trap.
//!
//! ## Mock-friendly
//!
//! `Reranker` accepts any [`Member`] including [`Member::Mock`], so
//! the entire `Rag` pipeline can be exercised in tests without
//! touching a real provider.

use crate::memory::vector::Hit;
use crate::swarm::budget::SharedDollarBudget;
use crate::swarm::member::Member;

use super::pipeline::RagErr;

/// LLM-as-reranker. Calls the wrapped [`Member`] once per query with
/// a structured prompt listing the candidates; parses the integer
/// scores back and rewrites each [`Hit::score`].
#[derive(Clone)]
pub struct Reranker {
    member: Member,
    /// Soft cap on how many candidates the reranker is asked to score
    /// in one shot. Beyond this we batch — the prompt blows out
    /// otherwise.
    batch_size: usize,
}

impl std::fmt::Debug for Reranker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reranker")
            .field("member", &self.member)
            .field("batch_size", &self.batch_size)
            .finish()
    }
}

impl Reranker {
    /// Build a reranker over `member`. Default batch size 20.
    pub fn new(member: Member) -> Self {
        Self {
            member,
            batch_size: 20,
        }
    }

    #[must_use]
    pub fn with_batch_size(mut self, n: usize) -> Self {
        self.batch_size = n.max(1);
        self
    }

    /// Borrow the underlying member.
    pub fn member(&self) -> &Member {
        &self.member
    }

    pub fn batch_size(&self) -> usize {
        self.batch_size
    }

    /// Re-score `hits` for `query`. Returns hits in **descending**
    /// score order with the reranker's scores overwriting the original
    /// cosine scores. On any error (network, parse, budget), the
    /// original hits are returned unchanged so the pipeline degrades
    /// gracefully.
    pub async fn rerank(
        &self,
        query: &str,
        hits: Vec<Hit>,
        budget: &SharedDollarBudget,
    ) -> Result<Vec<Hit>, RagErr> {
        if hits.is_empty() {
            return Ok(hits);
        }
        let mut scored: Vec<Hit> = Vec::with_capacity(hits.len());
        for chunk in hits.chunks(self.batch_size) {
            let prompt = build_prompt(query, chunk);
            match self.member.ask(&prompt, budget).await {
                Ok(reply) => {
                    let scores = parse_scores(&reply.body, chunk.len());
                    for (mut h, s) in chunk.iter().cloned().zip(scores) {
                        if let Some(score) = s {
                            h.score = score as f32 / 100.0;
                        }
                        scored.push(h);
                    }
                }
                Err(_) => {
                    // Soft fail: pass-through. Pipeline keeps moving.
                    scored.extend(chunk.iter().cloned());
                }
            }
        }
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(scored)
    }
}

fn build_prompt(query: &str, candidates: &[Hit]) -> String {
    let mut s = String::new();
    s.push_str("Score each candidate's relevance to the query on a 0-100 scale.\n");
    s.push_str("Reply with one line per candidate in the form `INDEX: SCORE`.\n\n");
    s.push_str("Query: ");
    s.push_str(query);
    s.push_str("\n\nCandidates:\n");
    for (i, h) in candidates.iter().enumerate() {
        s.push_str(&format!("[{i}] {}\n", truncate(&h.text, 400)));
    }
    s
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push_str("...");
        out
    }
}

/// Parse the reranker's reply into a per-candidate score. Returns
/// `None` for any candidate the reply didn't cover (the caller keeps
/// the original cosine score). Tolerant: accepts `INDEX: SCORE`,
/// `INDEX) SCORE`, `INDEX SCORE`, and ignores any other lines.
fn parse_scores(body: &str, n: usize) -> Vec<Option<u32>> {
    let mut out = vec![None; n];
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Find the first non-digit char to terminate the index, then
        // the first digit run after that for the score.
        let mut chars = line.chars().peekable();
        let mut idx_str = String::new();
        while let Some(c) = chars.peek() {
            if c.is_ascii_digit() {
                idx_str.push(*c);
                chars.next();
            } else {
                break;
            }
        }
        if idx_str.is_empty() {
            continue;
        }
        // Skip separator characters (`:`, `)`, `.`, whitespace, etc).
        while let Some(c) = chars.peek() {
            if c.is_ascii_digit() {
                break;
            }
            chars.next();
        }
        let mut score_str = String::new();
        while let Some(c) = chars.peek() {
            if c.is_ascii_digit() {
                score_str.push(*c);
                chars.next();
            } else {
                break;
            }
        }
        if score_str.is_empty() {
            continue;
        }
        let idx: usize = match idx_str.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let score: u32 = match score_str.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        if idx < n {
            out[idx] = Some(score.min(100));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::vector::Hit;
    use std::collections::HashMap;

    fn hit(id: &str, text: &str, score: f32) -> Hit {
        Hit {
            id: id.into(),
            text: text.into(),
            score,
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn rerank_uses_member_scores() {
        let reply = "0: 90\n1: 10\n2: 50";
        let r = Reranker::new(Member::mock("rk", reply, 1));
        let hits = vec![
            hit("a", "first", 0.3),
            hit("b", "second", 0.4),
            hit("c", "third", 0.5),
        ];
        let budget = SharedDollarBudget::new(1000);
        let out = r.rerank("q", hits, &budget).await.unwrap();
        // Reranker scores override cosine scores; expect order a, c, b.
        assert_eq!(out[0].id, "a");
        assert_eq!(out[1].id, "c");
        assert_eq!(out[2].id, "b");
    }

    #[tokio::test]
    async fn rerank_empty_passthrough() {
        let r = Reranker::new(Member::mock("rk", "", 1));
        let budget = SharedDollarBudget::new(1000);
        let out = r.rerank("q", Vec::new(), &budget).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn rerank_soft_fails_on_provider_error() {
        let r = Reranker::new(Member::mock_error("rk", "boom"));
        let hits = vec![hit("a", "first", 0.7)];
        let budget = SharedDollarBudget::new(1000);
        let out = r.rerank("q", hits, &budget).await.unwrap();
        // Original hits passed through.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "a");
        assert_eq!(out[0].score, 0.7);
    }

    #[test]
    fn parse_scores_tolerates_formats() {
        let body = "0: 100\n1) 50\n2 25\nignored garbage\n3: 999";
        let s = parse_scores(body, 4);
        assert_eq!(s[0], Some(100));
        assert_eq!(s[1], Some(50));
        assert_eq!(s[2], Some(25));
        assert_eq!(s[3], Some(100));
    }

    #[test]
    fn parse_scores_clamps_to_100() {
        let s = parse_scores("0: 250", 1);
        assert_eq!(s[0], Some(100));
    }
}
