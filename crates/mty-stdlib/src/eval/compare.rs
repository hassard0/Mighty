//! Comparison strategies + the [`Report`] shape.
//!
//! Three strategies, picked from the patterns that show up in real
//! LLM-eval workflows:
//!
//! - **Equal** — byte-equal string comparison after trimming + lower-
//!   casing. The strictest comparator; fine for "did the model emit
//!   the exact tool-name we expected" but useless on free-form prose.
//! - **SemanticSimilarity { threshold }** — cosine-similarity over
//!   stub-FNV embeddings (see [`crate::memory::Embedder`]). Two
//!   replies are *equivalent* if their cosine similarity exceeds the
//!   threshold. The stub embedder is bit-stable so eval reports
//!   reproduce across runs.
//! - **ToolCallSetEqual** — extracts every `@tool` invocation from
//!   each reply (parsed from `tool_name(...)` patterns the agent
//!   emits as part of the assistant text) and compares the *set*
//!   of tool names. Order-independent. Falls back to "no tool
//!   calls = trivially equal" so a non-tool reply doesn't fail this
//!   comparator just because the model declined to use a tool.

use std::collections::HashSet;
use std::sync::Arc;

use crate::memory::embeddings::{Embedder, StubEmbedder};

/// One comparator. Build via the named constructors
/// ([`Compare::equal`], [`Compare::semantic_similarity`],
/// [`Compare::tool_call_set_equal`]).
#[derive(Clone)]
pub enum Compare {
    /// Byte-equal after trim + lowercase.
    Equal,
    /// Cosine-similarity over stub embeddings; the two strings are
    /// considered equivalent if similarity >= threshold.
    SemanticSimilarity {
        threshold: f32,
        embedder: Arc<dyn Embedder>,
    },
    /// Compare the *set* of tool calls extracted from each reply
    /// (order-independent).
    ToolCallSetEqual,
}

impl std::fmt::Debug for Compare {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Compare::Equal => write!(f, "Compare::Equal"),
            Compare::SemanticSimilarity {
                threshold,
                embedder,
            } => write!(
                f,
                "Compare::SemanticSimilarity {{ threshold: {threshold}, embedder: {} }}",
                embedder.name()
            ),
            Compare::ToolCallSetEqual => write!(f, "Compare::ToolCallSetEqual"),
        }
    }
}

impl Compare {
    /// Strictest comparator — strings must match after trim + lower.
    pub fn equal() -> Self {
        Self::Equal
    }

    /// Cosine-similarity threshold comparator. `threshold` is clamped
    /// into `[0.0, 1.0]`. Uses the default stub embedder; for a real
    /// embedding backend pass it explicitly via
    /// [`Compare::semantic_similarity_with`].
    pub fn semantic_similarity(threshold: f32) -> Self {
        Self::SemanticSimilarity {
            threshold: threshold.clamp(0.0, 1.0),
            embedder: Arc::new(StubEmbedder::new()),
        }
    }

    /// Cosine-similarity comparator with a caller-supplied embedder.
    /// Used to swap in the OpenAI / qdrant backends without touching
    /// the eval driver.
    pub fn semantic_similarity_with(threshold: f32, embedder: Arc<dyn Embedder>) -> Self {
        Self::SemanticSimilarity {
            threshold: threshold.clamp(0.0, 1.0),
            embedder,
        }
    }

    /// Tool-call-set comparator. Two replies are equivalent iff their
    /// extracted tool-name sets match.
    pub fn tool_call_set_equal() -> Self {
        Self::ToolCallSetEqual
    }

    /// Strategy name for the report. Stable / loggable.
    pub fn name(&self) -> &'static str {
        match self {
            Compare::Equal => "equal",
            Compare::SemanticSimilarity { .. } => "semantic_similarity",
            Compare::ToolCallSetEqual => "tool_call_set_equal",
        }
    }

    /// Decide whether two replies are equivalent under this comparator.
    /// Stateless / pure.
    pub fn equivalent(&self, a: &str, b: &str) -> bool {
        match self {
            Compare::Equal => normalise_for_equal(a) == normalise_for_equal(b),
            Compare::SemanticSimilarity {
                threshold,
                embedder,
            } => {
                let Ok(va) = embedder.embed(a) else {
                    return false;
                };
                let Ok(vb) = embedder.embed(b) else {
                    return false;
                };
                cosine(&va, &vb) >= *threshold
            }
            Compare::ToolCallSetEqual => extract_tool_calls(a) == extract_tool_calls(b),
        }
    }
}

/// One reply's verdict against the baseline (or against the first
/// member's reply when there's no recorded baseline).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The reply matched the baseline under the configured comparator.
    Match,
    /// The reply did not match.
    Diverge,
    /// The member errored before producing a reply.
    Error,
    /// Only one member was registered — the comparator has nothing to
    /// compare *against*. Surfaced rather than auto-marked-Match so
    /// callers can opt to require ≥2 members.
    SingleMember,
}

/// One divergence record — used by the [`Report`] to surface "case X,
/// member Y disagreed with the baseline".
#[derive(Debug, Clone)]
pub struct Divergence {
    pub case_name: String,
    pub member_label: String,
    pub baseline: String,
    pub actual: String,
    /// Free-form reason: error string, or a short "cosine 0.42 below
    /// threshold 0.85" diagnostic.
    pub reason: String,
}

/// Final report. Carries every cell's verdict + the divergence rows
/// so a CI step can render a table without re-running the eval.
#[derive(Debug, Clone)]
pub struct Report {
    /// Suite name (echoed from [`super::Suite::name`]).
    pub suite_name: String,
    /// Comparator strategy used. `"equal"` / `"semantic_similarity"`
    /// / `"tool_call_set_equal"`.
    pub strategy: &'static str,
    /// `cells[i][j]` = verdict of `case[i]` × `member[j]`.
    pub cells: Vec<Vec<Verdict>>,
    /// Every (case, member) pair whose verdict was `Diverge` or
    /// `Error`. Empty when the eval passed.
    pub divergences: Vec<Divergence>,
    /// Case names, in suite-registration order.
    pub case_names: Vec<String>,
    /// Member labels, in registration order.
    pub member_labels: Vec<String>,
    /// Total cost (cents) spent across every dispatched member call.
    pub total_cost_cents: u64,
}

impl Report {
    /// True when every cell is `Match` (or `SingleMember`).
    pub fn passed(&self) -> bool {
        self.cells
            .iter()
            .flatten()
            .all(|v| matches!(v, Verdict::Match | Verdict::SingleMember))
    }

    /// Number of (case, member) pairs that diverged or errored.
    pub fn failure_count(&self) -> usize {
        self.cells
            .iter()
            .flatten()
            .filter(|v| matches!(v, Verdict::Diverge | Verdict::Error))
            .count()
    }

    /// Dollars (f64). The integer `total_cost_cents` is the source of
    /// truth.
    pub fn total_cost_dollars(&self) -> f64 {
        self.total_cost_cents as f64 / 100.0
    }

    /// Multi-line human-readable render. Useful for CLI output.
    pub fn render(&self) -> String {
        let mut out = format!(
            "eval `{}` ({}): {} case(s) × {} member(s)\n",
            self.suite_name,
            self.strategy,
            self.case_names.len(),
            self.member_labels.len()
        );
        if self.passed() {
            out.push_str("verdict: PASS\n");
        } else {
            out.push_str(&format!(
                "verdict: FAIL ({} divergence(s))\n",
                self.failure_count()
            ));
            for d in &self.divergences {
                out.push_str(&format!(
                    "  - [{}/{}] {}\n      baseline: {}\n      actual:   {}\n",
                    d.case_name, d.member_label, d.reason, d.baseline, d.actual
                ));
            }
        }
        out.push_str(&format!("cost: ${:.4}\n", self.total_cost_dollars()));
        out
    }
}

fn normalise_for_equal(s: &str) -> String {
    s.trim().to_lowercase()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>()
}

/// Extract tool-call names from a reply. Today we recognise two
/// shapes, both produced by the v0.27 `@tool` runtime:
///
/// 1. **Function-call literal:** `tool_name(arg1, arg2)` anywhere in
///    the reply text.
/// 2. **JSON sentinel:** `<tool_use name="tool_name">` (the format the
///    Anthropic streaming adapter emits when an assistant block was
///    a `ToolUse`).
///
/// We *only* extract names — the eval comparator runs on the *set* of
/// names; the argument shape is a v0.29 follow-up.
pub fn extract_tool_calls(reply: &str) -> HashSet<String> {
    let mut out = HashSet::new();

    // Shape 1: `<tool_use name="X">`. Robust to whitespace.
    let mut s = reply;
    while let Some(idx) = s.find("<tool_use") {
        s = &s[idx..];
        if let Some(name_start) = s.find("name=\"") {
            let after = &s[name_start + 6..];
            if let Some(end) = after.find('"') {
                let name = &after[..end];
                if !name.is_empty() {
                    out.insert(name.to_string());
                }
                s = &after[end..];
                continue;
            }
        }
        // Skip the marker so the loop can advance.
        s = &s[1..];
    }

    // Shape 2: bare `tool_name(...)` invocations. We only treat
    // identifiers followed *immediately* by `(` as a tool call to
    // keep the false-positive rate low — random English text rarely
    // contains `foo(`. Identifiers are `[a-z][a-z0-9_]*` (lower-
    // case-only matches the `@tool` naming convention).
    let bytes = reply.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_lowercase() {
            // Walk forward over identifier chars.
            let start = i;
            while i < bytes.len() {
                let cc = bytes[i];
                if cc.is_ascii_lowercase() || cc.is_ascii_digit() || cc == b'_' {
                    i += 1;
                } else {
                    break;
                }
            }
            if i < bytes.len() && bytes[i] == b'(' && i - start >= 3 {
                // Look-behind: skip when the identifier is preceded
                // by another identifier char (so `xfoo(` doesn't
                // double-count both `xfoo` and `foo`).
                let preceded_by_ident = start > 0
                    && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
                if !preceded_by_ident {
                    let name = std::str::from_utf8(&bytes[start..i])
                        .unwrap_or("")
                        .to_string();
                    if !name.is_empty() {
                        out.insert(name);
                    }
                }
            }
        } else {
            i += 1;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_matches_after_trim_and_lower() {
        let c = Compare::equal();
        assert!(c.equivalent("Paris", " paris\n"));
        assert!(c.equivalent("yes", "YES"));
        assert!(!c.equivalent("yes", "no"));
    }

    #[test]
    fn equal_strategy_name_is_stable() {
        assert_eq!(Compare::equal().name(), "equal");
    }

    #[test]
    fn semantic_similarity_threshold_clamps_to_unit_interval() {
        if let Compare::SemanticSimilarity { threshold, .. } = Compare::semantic_similarity(3.0) {
            assert!(threshold <= 1.0);
        } else {
            panic!("wrong variant");
        }
        if let Compare::SemanticSimilarity { threshold, .. } = Compare::semantic_similarity(-1.0) {
            assert!(threshold >= 0.0);
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn semantic_similarity_matches_token_overlap() {
        let c = Compare::semantic_similarity(0.5);
        assert!(c.equivalent("anthropic claude opus", "anthropic claude opus model"));
    }

    #[test]
    fn semantic_similarity_rejects_orthogonal_text() {
        let c = Compare::semantic_similarity(0.5);
        // Distinct token sets — should land below threshold.
        assert!(!c.equivalent(
            "completely unrelated alpha bravo",
            "different charlie delta echo"
        ));
    }

    #[test]
    fn semantic_similarity_zero_threshold_always_matches() {
        let c = Compare::semantic_similarity(0.0);
        // Even an empty-vs-empty embed pair stays at cosine 0; the
        // 0 threshold is the >= sentinel.
        assert!(c.equivalent("anything", "anything"));
    }

    #[test]
    fn tool_call_set_extracts_xml_marker() {
        let s =
            r#"I'll search: <tool_use name="search_web"> and read: <tool_use name="read_file">"#;
        let set = extract_tool_calls(s);
        assert!(set.contains("search_web"));
        assert!(set.contains("read_file"));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn tool_call_set_extracts_function_call() {
        let s = "Calling search_web(\"rust\") then read_file(\"a.txt\")";
        let set = extract_tool_calls(s);
        assert!(set.contains("search_web"));
        assert!(set.contains("read_file"));
    }

    #[test]
    fn tool_call_set_ignores_short_or_no_paren_idents() {
        let s = "hi there friends and ab(";
        // `ab` is < 3 chars so we skip it.
        let set = extract_tool_calls(s);
        assert!(set.is_empty());
    }

    #[test]
    fn tool_call_set_equal_compares_sets_not_order() {
        let c = Compare::tool_call_set_equal();
        assert!(c.equivalent(
            "first call_a() then call_b()",
            "first call_b() then call_a()"
        ));
    }

    #[test]
    fn tool_call_set_equal_falls_through_on_no_tools() {
        let c = Compare::tool_call_set_equal();
        // Two replies with no tool calls: both empty sets, trivially
        // equal.
        assert!(c.equivalent("just text here", "different just text"));
    }

    #[test]
    fn tool_call_set_equal_detects_set_difference() {
        let c = Compare::tool_call_set_equal();
        assert!(!c.equivalent("call_a()", "call_b()"));
    }

    #[test]
    fn report_passed_when_every_cell_matches() {
        let r = Report {
            suite_name: "x".into(),
            strategy: "equal",
            cells: vec![vec![Verdict::Match, Verdict::Match]],
            divergences: vec![],
            case_names: vec!["c1".into()],
            member_labels: vec!["m1".into(), "m2".into()],
            total_cost_cents: 0,
        };
        assert!(r.passed());
        assert_eq!(r.failure_count(), 0);
    }

    #[test]
    fn report_fails_when_one_cell_diverges() {
        let r = Report {
            suite_name: "x".into(),
            strategy: "equal",
            cells: vec![vec![Verdict::Match, Verdict::Diverge]],
            divergences: vec![Divergence {
                case_name: "c1".into(),
                member_label: "m2".into(),
                baseline: "a".into(),
                actual: "b".into(),
                reason: "not equal".into(),
            }],
            case_names: vec!["c1".into()],
            member_labels: vec!["m1".into(), "m2".into()],
            total_cost_cents: 5,
        };
        assert!(!r.passed());
        assert_eq!(r.failure_count(), 1);
        assert_eq!(r.total_cost_dollars(), 0.05);
        let rendered = r.render();
        assert!(rendered.contains("FAIL"));
        assert!(rendered.contains("c1"));
        assert!(rendered.contains("m2"));
    }

    #[test]
    fn report_render_pass_case() {
        let r = Report {
            suite_name: "demo".into(),
            strategy: "equal",
            cells: vec![vec![Verdict::Match]],
            divergences: vec![],
            case_names: vec!["c1".into()],
            member_labels: vec!["m1".into()],
            total_cost_cents: 100,
        };
        let s = r.render();
        assert!(s.contains("PASS"));
        assert!(s.contains("$1.00"));
    }

    #[test]
    fn report_single_member_counts_as_pass() {
        let r = Report {
            suite_name: "x".into(),
            strategy: "equal",
            cells: vec![vec![Verdict::SingleMember]],
            divergences: vec![],
            case_names: vec!["c1".into()],
            member_labels: vec!["m1".into()],
            total_cost_cents: 0,
        };
        assert!(r.passed());
    }

    #[test]
    fn cosine_matches_dot_product_on_unit_vectors() {
        // StubEmbedder outputs L2-normalised vectors, so cosine ==
        // dot product. Cross-check against a hand-rolled dot.
        let e = StubEmbedder::new();
        let a = e.embed("hello world").unwrap();
        let b = e.embed("hello world").unwrap();
        assert!((cosine(&a, &b) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn cosine_of_mismatched_dims_returns_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert_eq!(cosine(&a, &b), 0.0);
    }
}
