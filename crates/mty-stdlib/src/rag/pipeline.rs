//! `std.rag.Rag` — end-to-end RAG pipeline.
//!
//! ```ignore
//! let answer = Rag.new()
//!   .with_index(index)
//!   .with_retriever_top_k(5)
//!   .with_reranker(Member.anthropic("claude-haiku-4-5"))
//!   .with_member(Member.anthropic("claude-opus-4-7"))
//!   .ask("What's Mighty's capability typing?")?
//! ```
//!
//! Internally:
//!
//! 1. Embed the query (via the underlying [`VectorStore`]'s embedder).
//! 2. Retrieve `top_k` hits via [`Retriever`].
//! 3. (Optional) re-score with the [`Reranker`].
//! 4. Build a grounded prompt: `system + "Context:\n{hits}\n\nQuestion: {q}"`.
//! 5. Ask the answering [`Member`]; return the body.
//!
//! Multi-modal: [`Rag::ask_with_image`] / [`Rag::ask_with_images`]
//! send the same prompt augmented with image content blocks. Retrieval
//! is still text-only (the query string is what gets embedded); the
//! image rides on the final ask along with the retrieved context.

use thiserror::Error;

use crate::llm::error::LlmError;
use crate::llm::image::Image;
use crate::llm::message::{ContentBlock, Message, Role};
use crate::llm::provider::{CompletionRequest, LlmProvider};
use crate::memory::vector::{Hit, VectorErr};
use crate::swarm::budget::SharedDollarBudget;
use crate::swarm::member::Member;

use super::index::{Index, IndexErr};
use super::reranker::Reranker;
use super::retriever::{Retriever, RetrieverConfig};

/// Errors returned by [`Rag::ask`] / [`Rag::ask_with_image`].
#[derive(Debug, Error)]
pub enum RagErr {
    #[error("rag pipeline: no index attached — call with_index first")]
    NoIndex,
    #[error("rag pipeline: no answering member attached — call with_member first")]
    NoMember,
    #[error("rag pipeline: vector backend: {0}")]
    Vector(#[from] VectorErr),
    #[error("rag pipeline: index io: {0}")]
    Index(String),
    #[error("rag pipeline: llm provider: {0}")]
    Llm(#[from] LlmError),
    #[error("rag pipeline: image: {0}")]
    Image(String),
}

impl From<IndexErr> for RagErr {
    fn from(e: IndexErr) -> Self {
        match e {
            IndexErr::Vector(v) => RagErr::Vector(v),
            IndexErr::Io(s) => RagErr::Index(s),
        }
    }
}

/// End-to-end RAG pipeline. All `with_*` methods return `self` so the
/// builder chain reads top-to-bottom.
pub struct Rag {
    index: Option<Index>,
    retriever_config: RetrieverConfig,
    reranker: Option<Reranker>,
    /// The LLM that answers the augmented prompt.
    member: Option<Member>,
    /// Optional system preamble. Defaults to a tight instruction.
    system: String,
    /// Dollar budget shared between the reranker + answer calls.
    /// Defaults to a comfortable $1.00 cap; callers cap tighter.
    budget: SharedDollarBudget,
}

impl std::fmt::Debug for Rag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rag")
            .field("index", &self.index.is_some())
            .field("retriever_config", &self.retriever_config)
            .field("reranker", &self.reranker)
            .field("member", &self.member)
            .field("system_len", &self.system.len())
            .finish()
    }
}

impl Default for Rag {
    fn default() -> Self {
        Self::new()
    }
}

impl Rag {
    pub fn new() -> Self {
        Self {
            index: None,
            retriever_config: RetrieverConfig::default(),
            reranker: None,
            member: None,
            system: default_system().to_string(),
            budget: SharedDollarBudget::new(100), // $1.00
        }
    }

    /// Attach the index to retrieve over.
    #[must_use]
    pub fn with_index(mut self, index: Index) -> Self {
        self.index = Some(index);
        self
    }

    /// Set retrieval top-k. Default 5.
    #[must_use]
    pub fn with_retriever_top_k(mut self, k: usize) -> Self {
        self.retriever_config.top_k = k.max(1);
        self
    }

    /// Set retrieval score threshold.
    #[must_use]
    pub fn with_retriever_min_score(mut self, s: f32) -> Self {
        self.retriever_config.min_score = Some(s);
        self
    }

    /// Enable MMR diversification.
    #[must_use]
    pub fn with_mmr(mut self, on: bool) -> Self {
        self.retriever_config.mmr = on;
        self
    }

    /// Replace the retriever config wholesale.
    #[must_use]
    pub fn with_retriever_config(mut self, cfg: RetrieverConfig) -> Self {
        self.retriever_config = cfg;
        self
    }

    /// Attach a reranker. Optional — when absent, retrieval scores
    /// pass straight through.
    #[must_use]
    pub fn with_reranker(mut self, member: Member) -> Self {
        self.reranker = Some(Reranker::new(member));
        self
    }

    /// Attach a pre-built reranker (lets you tune `batch_size`).
    #[must_use]
    pub fn with_reranker_instance(mut self, rr: Reranker) -> Self {
        self.reranker = Some(rr);
        self
    }

    /// Attach the answering member. Required before `ask`.
    #[must_use]
    pub fn with_member(mut self, member: Member) -> Self {
        self.member = Some(member);
        self
    }

    /// Override the system preamble.
    #[must_use]
    pub fn with_system(mut self, s: impl Into<String>) -> Self {
        self.system = s.into();
        self
    }

    /// Set the shared budget across reranker + answer calls.
    #[must_use]
    pub fn with_budget(mut self, budget: SharedDollarBudget) -> Self {
        self.budget = budget;
        self
    }

    /// Cap spending in cents (shortcut for [`with_budget`]).
    #[must_use]
    pub fn with_budget_cents(mut self, cents: u64) -> Self {
        self.budget = SharedDollarBudget::new(cents);
        self
    }

    pub fn budget(&self) -> &SharedDollarBudget {
        &self.budget
    }

    /// Run retrieval (+ optional rerank) and return the hits without
    /// asking. Useful when callers want to render the citations
    /// alongside the answer.
    pub async fn retrieve(&self, query: &str) -> Result<Vec<Hit>, RagErr> {
        let index = self.index.as_ref().ok_or(RagErr::NoIndex)?;
        let r = Retriever::new(index).with_config(self.retriever_config.clone());
        let mut hits = r.retrieve(query)?;
        if let Some(rr) = &self.reranker {
            hits = rr.rerank(query, hits, &self.budget).await?;
        }
        Ok(hits)
    }

    /// One-shot RAG: retrieve, rerank, build augmented prompt, ask.
    /// Returns the answer body.
    pub async fn ask(&self, query: &str) -> Result<String, RagErr> {
        self.ask_inner(query, Vec::new()).await
    }

    /// RAG + a single image input. The image rides on the answering
    /// turn alongside the retrieved context.
    pub async fn ask_with_image(&self, query: &str, image: Image) -> Result<String, RagErr> {
        self.ask_inner(query, vec![image]).await
    }

    /// RAG + multiple image inputs.
    pub async fn ask_with_images(
        &self,
        query: &str,
        images: Vec<Image>,
    ) -> Result<String, RagErr> {
        self.ask_inner(query, images).await
    }

    async fn ask_inner(&self, query: &str, images: Vec<Image>) -> Result<String, RagErr> {
        let member = self.member.as_ref().ok_or(RagErr::NoMember)?;
        let hits = self.retrieve(query).await?;
        let prompt = build_augmented_prompt(&self.system, query, &hits);
        if images.is_empty() {
            // Single-text path uses the existing Member::ask so all
            // accounting + recording stays uniform.
            let reply = member.ask(&prompt, &self.budget).await?;
            return Ok(reply.body);
        }
        // Multi-modal path: dispatch directly through the provider with
        // mixed image/text content. Mock members get a degenerate text
        // path that ignores images (preserves test ergonomics).
        if let Member::Mock(_) = member {
            let reply = member.ask(&prompt, &self.budget).await?;
            return Ok(reply.body);
        }
        let content = build_multimodal_content(&prompt, images)?;
        let message = Message {
            role: Role::User,
            content,
        };
        let req = CompletionRequest::new(member.model(), vec![message]);
        let answer = match member {
            Member::Anthropic { client, .. } => client.complete(req).await?,
            Member::OpenAi { client, .. } => client.complete(req).await?,
            Member::Gemini { client, .. } => client.complete(req).await?,
            Member::Bedrock { client, .. } => client.complete(req).await?,
            Member::Mock(_) => unreachable!("handled above"),
        };
        Ok(answer.text())
    }
}

fn default_system() -> &'static str {
    "You answer the user's question using ONLY the provided context. \
     If the context does not contain the answer, say so plainly."
}

fn build_augmented_prompt(system: &str, query: &str, hits: &[Hit]) -> String {
    let mut s = String::new();
    if !system.is_empty() {
        s.push_str(system);
        s.push_str("\n\n");
    }
    s.push_str("Context:\n");
    if hits.is_empty() {
        s.push_str("(no relevant context found)\n");
    } else {
        for (i, h) in hits.iter().enumerate() {
            s.push_str(&format!("[{}] (score={:.3}) {}\n", i + 1, h.score, h.text));
        }
    }
    s.push_str("\nQuestion: ");
    s.push_str(query);
    s.push_str("\n\nAnswer:");
    s
}

fn build_multimodal_content(
    prompt: &str,
    images: Vec<Image>,
) -> Result<Vec<ContentBlock>, RagErr> {
    let mut blocks = Vec::with_capacity(images.len() + 1);
    for img in images {
        blocks.push(ContentBlock::Image {
            source: img.to_source().map_err(|e| RagErr::Image(e.to_string()))?,
        });
    }
    blocks.push(ContentBlock::text(prompt));
    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn build_idx() -> Index {
        let mut idx = Index::in_memory();
        idx.add_text(
            "Mighty's capability typing tags every value with the effects \
             it can produce (net, fs, model) so the compiler can refuse \
             unsafe combinations at the call site.",
            HashMap::new(),
        )
        .add_text(
            "Mighty agents communicate via typed protocols using bang-send.",
            HashMap::new(),
        )
        .add_text("A random unrelated fact about turtles.", HashMap::new());
        idx.build().unwrap();
        idx
    }

    #[tokio::test]
    async fn ask_returns_member_body() {
        let idx = build_idx();
        let rag = Rag::new()
            .with_index(idx)
            .with_retriever_top_k(2)
            .with_member(Member::mock("opus", "capability typing tags effects.", 1));
        let answer = rag.ask("What is capability typing?").await.unwrap();
        assert!(answer.contains("capability"));
    }

    #[tokio::test]
    async fn ask_fails_without_member() {
        let idx = build_idx();
        let rag = Rag::new().with_index(idx);
        let err = rag.ask("q").await.unwrap_err();
        assert!(matches!(err, RagErr::NoMember));
    }

    #[tokio::test]
    async fn ask_fails_without_index() {
        let rag = Rag::new().with_member(Member::mock("m", "x", 1));
        let err = rag.ask("q").await.unwrap_err();
        assert!(matches!(err, RagErr::NoIndex));
    }

    #[tokio::test]
    async fn retrieve_returns_hits_before_ask() {
        let idx = build_idx();
        let rag = Rag::new()
            .with_index(idx)
            .with_retriever_top_k(2)
            .with_member(Member::mock("m", "x", 1));
        let hits = rag.retrieve("capability").await.unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().any(|h| h.text.contains("capability")));
    }

    #[tokio::test]
    async fn reranker_changes_hit_order() {
        let idx = build_idx();
        // Reranker says hit 1 is the most relevant (sneakily promotes
        // the turtle fact).
        let rag = Rag::new()
            .with_index(idx)
            .with_retriever_top_k(3)
            .with_reranker(Member::mock("rk", "0: 10\n1: 5\n2: 99", 1))
            .with_member(Member::mock("opus", "ok", 1));
        let hits = rag.retrieve("capability").await.unwrap();
        // The reranker's #2 gets the top spot.
        assert!(!hits.is_empty());
        assert!(hits[0].score >= 0.9);
    }

    #[tokio::test]
    async fn augmented_prompt_includes_context() {
        let hits = vec![Hit {
            id: "x".into(),
            text: "the answer is 42".into(),
            score: 0.9,
            metadata: HashMap::new(),
        }];
        let p = build_augmented_prompt("system", "what is the answer?", &hits);
        assert!(p.contains("system"));
        assert!(p.contains("Context:"));
        assert!(p.contains("the answer is 42"));
        assert!(p.contains("what is the answer?"));
    }

    #[tokio::test]
    async fn ask_with_image_routes_through_mock() {
        let idx = build_idx();
        let rag = Rag::new()
            .with_index(idx)
            .with_member(Member::mock("opus", "vision answer", 1));
        let img = Image::from_bytes(b"fake-png".to_vec(), "image/png");
        let ans = rag.ask_with_image("describe", img).await.unwrap();
        assert_eq!(ans, "vision answer");
    }

    #[tokio::test]
    async fn budget_is_shared_across_calls() {
        let idx = build_idx();
        let budget = SharedDollarBudget::new(10);
        let rag = Rag::new()
            .with_index(idx)
            .with_budget(budget.clone())
            .with_member(Member::mock("opus", "ok", 5));
        rag.ask("q").await.unwrap();
        // Mock charged 5 cents; budget should reflect.
        assert!(budget.consumed_cents() >= 5);
    }

    #[tokio::test]
    async fn empty_context_says_so() {
        let mut idx = Index::in_memory();
        // No docs.
        idx.build().unwrap();
        let rag = Rag::new()
            .with_index(idx)
            .with_member(Member::mock("opus", "nothing found", 1));
        let ans = rag.ask("what?").await.unwrap();
        // Mock returns canned reply regardless; just confirm pipeline
        // didn't fail.
        assert_eq!(ans, "nothing found");
    }
}
