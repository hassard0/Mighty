//! Aggregations over the `observations` table — total cost,
//! per-{provider,model,agent} breakdown, p50/p95/p99 latency.
//!
//! Pure-rust over the snapshot vector so this module compiles even
//! without the `observe-sqlite` feature; the `mty inspect --cost`
//! CLI invokes [`summarize`] on whatever the active store hands
//! back.

use crate::observe::observation::{now_ms, LlmObservation};
use std::collections::BTreeMap;

/// Time window for `mty inspect --cost --since <D>`.
///
/// `Last(d)` = "the last `d` of wall-clock time before now". `All`
/// returns everything in the store.
#[derive(Debug, Clone, Copy)]
pub enum Window {
    Last { millis: u64 },
    All,
}

impl Window {
    /// Parse a `Go`-style duration spec: `7d`, `12h`, `30m`, `45s`,
    /// `500ms`. Returns `Err(String)` on malformed input — callers
    /// in the CLI surface that as a clean error.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let spec = spec.trim();
        if spec == "all" || spec.is_empty() {
            return Ok(Self::All);
        }
        // Split off the trailing unit.
        let (num_part, unit) = if let Some(num_part) = spec.strip_suffix("ms") {
            (num_part, "ms")
        } else if let Some(num_part) = spec.strip_suffix('s') {
            (num_part, "s")
        } else if let Some(num_part) = spec.strip_suffix('m') {
            (num_part, "m")
        } else if let Some(num_part) = spec.strip_suffix('h') {
            (num_part, "h")
        } else if let Some(num_part) = spec.strip_suffix('d') {
            (num_part, "d")
        } else {
            return Err(format!(
                "duration {spec:?} must end in ms/s/m/h/d or be `all`"
            ));
        };
        let n: u64 = num_part
            .parse()
            .map_err(|e| format!("duration {spec:?}: {e}"))?;
        let millis = match unit {
            "ms" => n,
            "s" => n.saturating_mul(1_000),
            "m" => n.saturating_mul(60_000),
            "h" => n.saturating_mul(3_600_000),
            "d" => n.saturating_mul(86_400_000),
            _ => unreachable!(),
        };
        Ok(Self::Last { millis })
    }

    /// Filter an iterator of observations by this window.
    pub fn filter<'a, I>(self, iter: I) -> Vec<&'a LlmObservation>
    where
        I: IntoIterator<Item = &'a LlmObservation>,
    {
        match self {
            Self::All => iter.into_iter().collect(),
            Self::Last { millis } => {
                let cutoff = now_ms().saturating_sub(millis);
                iter.into_iter()
                    .filter(|o| o.started_at_ms >= cutoff)
                    .collect()
            }
        }
    }
}

/// Group key for `mty inspect --cost --by <KEY>`.
#[derive(Debug, Clone, Copy)]
pub enum GroupBy {
    Provider,
    Model,
    Agent,
    None,
}

impl GroupBy {
    pub fn parse(spec: &str) -> Result<Self, String> {
        match spec.trim().to_ascii_lowercase().as_str() {
            "provider" => Ok(Self::Provider),
            "model" => Ok(Self::Model),
            "agent" => Ok(Self::Agent),
            "none" | "" => Ok(Self::None),
            other => Err(format!(
                "unknown group key {other:?} — try one of provider/model/agent/none"
            )),
        }
    }
}

/// One row in a `--by` rollup.
#[derive(Debug, Clone, PartialEq)]
pub struct AggregateRow {
    pub key: String,
    pub call_count: u64,
    pub total_cost_cents: i64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub p50_latency_ms: u64,
    pub p95_latency_ms: u64,
    pub p99_latency_ms: u64,
}

/// p50/p95/p99 latencies across an observation slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatencyPercentiles {
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub p99_ms: u64,
}

/// Top-level `mty inspect --cost` summary.
#[derive(Debug, Clone)]
pub struct CostSummary {
    pub window: Window,
    pub call_count: u64,
    pub total_cost_cents: i64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub latency: LatencyPercentiles,
    /// Always grouped — defaults to by-provider for the bare `--cost`
    /// invocation.
    pub by_group: Vec<AggregateRow>,
    /// Most-expensive single calls (default top 0; CLI passes `--top`).
    pub top_calls: Vec<LlmObservation>,
}

/// Compute the canonical `mty inspect --cost` summary over `obs`.
///
/// `group_by = None` collapses every call into one synthetic group
/// keyed `"all"`. `top_n = 0` skips the per-call top list.
pub fn summarize(
    obs: &[LlmObservation],
    window: Window,
    group_by: GroupBy,
    top_n: usize,
) -> CostSummary {
    let filtered = window.filter(obs.iter());

    let mut total_cost_cents: i64 = 0;
    let mut total_prompt: u64 = 0;
    let mut total_completion: u64 = 0;
    let mut latencies: Vec<u64> = Vec::with_capacity(filtered.len());
    for o in &filtered {
        total_cost_cents = total_cost_cents.saturating_add(o.cost_cents);
        total_prompt = total_prompt.saturating_add(o.prompt_tokens);
        total_completion = total_completion.saturating_add(o.completion_tokens);
        latencies.push(o.latency_ms);
    }

    let latency = percentiles(&latencies);
    let by_group = aggregate_by(&filtered, group_by);

    let mut top_calls: Vec<LlmObservation> = Vec::new();
    if top_n > 0 {
        let mut owned: Vec<LlmObservation> = filtered.iter().map(|o| (*o).clone()).collect();
        owned.sort_by_key(|o| std::cmp::Reverse(o.cost_cents));
        owned.truncate(top_n);
        top_calls = owned;
    }

    CostSummary {
        window,
        call_count: filtered.len() as u64,
        total_cost_cents,
        total_prompt_tokens: total_prompt,
        total_completion_tokens: total_completion,
        latency,
        by_group,
        top_calls,
    }
}

/// Group + sum a slice of observations.
pub fn aggregate_by(obs: &[&LlmObservation], by: GroupBy) -> Vec<AggregateRow> {
    let mut buckets: BTreeMap<String, Vec<&LlmObservation>> = BTreeMap::new();
    for o in obs {
        let key = match by {
            GroupBy::Provider => o.provider.clone(),
            GroupBy::Model => o.model.clone(),
            GroupBy::Agent => o
                .agent_id
                .map(|a| format!("agent#{a}"))
                .unwrap_or_else(|| "(no-agent)".to_string()),
            GroupBy::None => "all".to_string(),
        };
        buckets.entry(key).or_default().push(*o);
    }
    let mut rows: Vec<AggregateRow> = buckets
        .into_iter()
        .map(|(k, items)| {
            let mut total_cost_cents: i64 = 0;
            let mut total_prompt: u64 = 0;
            let mut total_completion: u64 = 0;
            let mut lats: Vec<u64> = Vec::with_capacity(items.len());
            for o in &items {
                total_cost_cents = total_cost_cents.saturating_add(o.cost_cents);
                total_prompt = total_prompt.saturating_add(o.prompt_tokens);
                total_completion = total_completion.saturating_add(o.completion_tokens);
                lats.push(o.latency_ms);
            }
            let p = percentiles(&lats);
            AggregateRow {
                key: k,
                call_count: items.len() as u64,
                total_cost_cents,
                total_prompt_tokens: total_prompt,
                total_completion_tokens: total_completion,
                p50_latency_ms: p.p50_ms,
                p95_latency_ms: p.p95_ms,
                p99_latency_ms: p.p99_ms,
            }
        })
        .collect();
    // Sort by descending cost — the dashboard reads top-down.
    rows.sort_by_key(|r| std::cmp::Reverse(r.total_cost_cents));
    rows
}

/// p50/p95/p99 over a slice of latencies. Empty slice → zeros.
/// We use the **nearest-rank** method: `p` percentile = element at
/// `ceil(p/100 * N) - 1`. Good enough for the cost-dashboard use
/// case and avoids floating-point interpolation.
pub fn percentiles(samples: &[u64]) -> LatencyPercentiles {
    if samples.is_empty() {
        return LatencyPercentiles {
            p50_ms: 0,
            p95_ms: 0,
            p99_ms: 0,
        };
    }
    let mut sorted: Vec<u64> = samples.to_vec();
    sorted.sort_unstable();
    let pick = |p: u64| -> u64 {
        // ceil(p/100 * N) - 1
        let n = sorted.len() as u64;
        let idx = ((p * n).div_ceil(100)).saturating_sub(1);
        sorted[idx as usize]
    };
    LatencyPercentiles {
        p50_ms: pick(50),
        p95_ms: pick(95),
        p99_ms: pick(99),
    }
}

#[cfg(test)]
#[allow(clippy::useless_vec)] // arrays don't impl IntoIterator<Item=T> the same way
mod tests {
    use super::*;

    fn obs_at(provider: &str, model: &str, cost: i64, latency: u64, age_ms: u64) -> LlmObservation {
        let mut o = LlmObservation::new(provider, model, 0, 0, latency);
        o.cost_cents = cost;
        o.started_at_ms = now_ms().saturating_sub(age_ms);
        o
    }

    #[test]
    fn window_parse_recognises_units() {
        assert!(matches!(Window::parse("all").unwrap(), Window::All));
        let cases = [
            ("500ms", 500u64),
            ("3s", 3_000),
            ("10m", 600_000),
            ("2h", 7_200_000),
            ("1d", 86_400_000),
        ];
        for (s, expect) in cases {
            match Window::parse(s).unwrap() {
                Window::Last { millis } => assert_eq!(millis, expect, "{s}"),
                _ => panic!("expected Last for {s}"),
            }
        }
    }

    #[test]
    fn window_parse_rejects_garbage() {
        assert!(Window::parse("3x").is_err());
        assert!(Window::parse("abc").is_err());
    }

    #[test]
    fn window_filters_by_age() {
        let obs = vec![
            obs_at("anthropic", "claude-opus", 100, 10, 0),
            obs_at("anthropic", "claude-opus", 100, 10, 60_000), // 1m ago
            obs_at("anthropic", "claude-opus", 100, 10, 86_400_000), // 1d ago
        ];
        let kept = Window::Last { millis: 120_000 }.filter(obs.iter());
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn percentiles_basic() {
        let p = percentiles(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        // nearest-rank: p50 = ceil(0.5*10)-1 = 4 -> idx 4 -> 5
        // p95 = ceil(0.95*10)-1 = 9 -> 10
        // p99 = ceil(0.99*10)-1 = 9 -> 10
        assert_eq!(p.p50_ms, 5);
        assert_eq!(p.p95_ms, 10);
        assert_eq!(p.p99_ms, 10);
    }

    #[test]
    fn percentiles_empty_returns_zeros() {
        let p = percentiles(&[]);
        assert_eq!(p.p50_ms, 0);
        assert_eq!(p.p95_ms, 0);
        assert_eq!(p.p99_ms, 0);
    }

    #[test]
    fn group_by_provider_sums_costs() {
        let obs = vec![
            obs_at("anthropic", "claude-opus", 100, 10, 0),
            obs_at("anthropic", "claude-sonnet", 50, 20, 0),
            obs_at("openai", "gpt-5", 25, 5, 0),
        ];
        let refs: Vec<&LlmObservation> = obs.iter().collect();
        let rows = aggregate_by(&refs, GroupBy::Provider);
        assert_eq!(rows.len(), 2);
        // Sorted by cost desc, so anthropic first.
        assert_eq!(rows[0].key, "anthropic");
        assert_eq!(rows[0].total_cost_cents, 150);
        assert_eq!(rows[1].key, "openai");
        assert_eq!(rows[1].total_cost_cents, 25);
    }

    #[test]
    fn group_by_model_keys_on_model() {
        let obs = vec![
            obs_at("anthropic", "claude-opus", 100, 10, 0),
            obs_at("anthropic", "claude-opus", 50, 20, 0),
            obs_at("anthropic", "claude-sonnet", 25, 5, 0),
        ];
        let refs: Vec<&LlmObservation> = obs.iter().collect();
        let rows = aggregate_by(&refs, GroupBy::Model);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key, "claude-opus");
        assert_eq!(rows[0].call_count, 2);
        assert_eq!(rows[0].total_cost_cents, 150);
    }

    #[test]
    fn group_by_agent_uses_no_agent_sentinel() {
        let mut o1 = obs_at("anthropic", "claude-opus", 100, 10, 0);
        o1.agent_id = Some(7);
        let o2 = obs_at("anthropic", "claude-opus", 50, 10, 0);
        let v = vec![o1, o2];
        let refs: Vec<&LlmObservation> = v.iter().collect();
        let rows = aggregate_by(&refs, GroupBy::Agent);
        // 2 distinct keys: agent#7 and (no-agent)
        assert_eq!(rows.len(), 2);
        let agent_row = rows.iter().find(|r| r.key == "agent#7").unwrap();
        assert_eq!(agent_row.total_cost_cents, 100);
    }

    #[test]
    fn summarize_top_n_returns_most_expensive() {
        let obs = vec![
            obs_at("anthropic", "claude-opus", 100, 10, 0),
            obs_at("anthropic", "claude-opus", 500, 10, 0),
            obs_at("anthropic", "claude-opus", 50, 10, 0),
            obs_at("anthropic", "claude-opus", 200, 10, 0),
        ];
        let s = summarize(&obs, Window::All, GroupBy::Provider, 2);
        assert_eq!(s.top_calls.len(), 2);
        assert_eq!(s.top_calls[0].cost_cents, 500);
        assert_eq!(s.top_calls[1].cost_cents, 200);
    }

    #[test]
    fn summarize_empty_observations_returns_zeros() {
        let s = summarize(&[], Window::All, GroupBy::Provider, 5);
        assert_eq!(s.call_count, 0);
        assert_eq!(s.total_cost_cents, 0);
        assert!(s.by_group.is_empty());
        assert!(s.top_calls.is_empty());
        assert_eq!(s.latency.p50_ms, 0);
    }

    #[test]
    fn summarize_aggregates_across_window() {
        let obs = vec![
            obs_at("anthropic", "claude-opus", 100, 10, 0),
            obs_at("anthropic", "claude-opus", 100, 20, 60_000), // 1m ago
            obs_at("anthropic", "claude-opus", 100, 30, 86_400_000), // 1d ago
        ];
        let s = summarize(&obs, Window::Last { millis: 120_000 }, GroupBy::Provider, 0);
        assert_eq!(s.call_count, 2);
        assert_eq!(s.total_cost_cents, 200);
    }

    #[test]
    fn group_by_parse() {
        assert!(matches!(
            GroupBy::parse("provider").unwrap(),
            GroupBy::Provider
        ));
        assert!(matches!(GroupBy::parse("MODEL").unwrap(), GroupBy::Model));
        assert!(matches!(GroupBy::parse("agent").unwrap(), GroupBy::Agent));
        assert!(GroupBy::parse("nope").is_err());
    }
}
