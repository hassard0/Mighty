//! Per-provider, per-model cost-per-token table.
//!
//! Numbers are baked in as **cents per million tokens** (integer)
//! so the cost math never touches `f64`. The shipped table is a
//! snapshot of 2026-05 public pricing — drift is the rule, so we
//! also support an override file pulled from `MTY_PRICING_OVERRIDE`
//! that wins ahead of the baked-in numbers.
//!
//! ## Override file format
//!
//! TOML with one `[[model]]` block per entry. Match is by
//! canonical prefix (longest-match-wins), same convention as the
//! v0.26 budget table.
//!
//! ```toml
//! [[model]]
//! prefix = "claude-opus"
//! input_cents_per_million = 1500
//! output_cents_per_million = 7500
//!
//! [[model]]
//! prefix = "gpt-5"
//! input_cents_per_million = 500
//! output_cents_per_million = 1500
//! ```
//!
//! ## Sources (2026-05 snapshot)
//!
//! - Anthropic: Opus 4.7 $15/$75 per Mtok, Sonnet 4.6 $3/$15,
//!   Haiku 4.5 $1/$5 — published `claude.com/pricing` page.
//! - OpenAI: GPT-5 list price has not been finalised in public docs
//!   as of 2026-05; we ship $5/$15 as a placeholder + TODO and
//!   recommend `MTY_PRICING_OVERRIDE` until a stable rate lands.
//! - Google: Gemini 2.5 Pro $1.25/$5, Flash $0.075/$0.30.
//! - AWS Bedrock: same as the upstream model. The model id encodes
//!   the family (e.g. `anthropic.claude-opus-4-7-v1:0`) so the
//!   prefix-match collapses to the right row.

use std::path::Path;

/// One row in the pricing table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricingRow {
    /// Match by `model.starts_with(prefix)`. Sorted longest-first
    /// so `claude-opus-4-7` doesn't accidentally match `claude-`.
    pub prefix: String,
    pub input_cents_per_million: u64,
    pub output_cents_per_million: u64,
}

/// In-memory pricing table.
///
/// Default `PricingTable::baked_in()` reflects the 2026-05 snapshot.
/// `from_override_path` overlays a TOML file on top — entries with
/// matching prefixes replace the baked-in row.
#[derive(Debug, Clone)]
pub struct PricingTable {
    rows: Vec<PricingRow>,
}

impl Default for PricingTable {
    fn default() -> Self {
        Self::baked_in()
    }
}

impl PricingTable {
    /// The shipped 2026-05 snapshot. Sorted longest-prefix-first so
    /// the lookup loop falls through correctly.
    pub fn baked_in() -> Self {
        let mut rows = vec![
            // Anthropic — Opus 4.7 / Sonnet 4.6 / Haiku 4.5
            PricingRow {
                prefix: "claude-opus".into(),
                input_cents_per_million: 1500,
                output_cents_per_million: 7500,
            },
            PricingRow {
                prefix: "claude-sonnet".into(),
                input_cents_per_million: 300,
                output_cents_per_million: 1500,
            },
            PricingRow {
                prefix: "claude-haiku".into(),
                input_cents_per_million: 100,
                output_cents_per_million: 500,
            },
            // Bedrock-hosted Anthropic — model ids are namespaced.
            // Prefix-match still works because every Bedrock id ends
            // in the upstream model name; we keep dedicated rows so
            // the override doc lines up with what callers actually
            // type.
            PricingRow {
                prefix: "anthropic.claude-opus".into(),
                input_cents_per_million: 1500,
                output_cents_per_million: 7500,
            },
            PricingRow {
                prefix: "anthropic.claude-sonnet".into(),
                input_cents_per_million: 300,
                output_cents_per_million: 1500,
            },
            PricingRow {
                prefix: "anthropic.claude-haiku".into(),
                input_cents_per_million: 100,
                output_cents_per_million: 500,
            },
            // OpenAI — GPT-5 public price TBD; ship a placeholder.
            // TODO(v0.31): update when openai.com publishes a stable rate.
            PricingRow {
                prefix: "gpt-5".into(),
                input_cents_per_million: 500,
                output_cents_per_million: 1500,
            },
            PricingRow {
                prefix: "gpt-4.1".into(),
                input_cents_per_million: 200,
                output_cents_per_million: 800,
            },
            PricingRow {
                prefix: "gpt-4o-mini".into(),
                input_cents_per_million: 15,
                output_cents_per_million: 60,
            },
            PricingRow {
                prefix: "gpt-4o".into(),
                input_cents_per_million: 250,
                output_cents_per_million: 1000,
            },
            // Google Gemini — 2.5 Pro / Flash.
            PricingRow {
                prefix: "gemini-2.5-pro".into(),
                input_cents_per_million: 125,
                output_cents_per_million: 500,
            },
            PricingRow {
                prefix: "gemini-2.5-flash".into(),
                input_cents_per_million: 8,
                output_cents_per_million: 30,
            },
        ];
        // Stable sort, longest prefix first so `gpt-4o-mini` wins
        // over `gpt-4o` for the same model id.
        rows.sort_by_key(|r| std::cmp::Reverse(r.prefix.len()));
        Self { rows }
    }

    /// Overlay an override file. Unknown prefixes ADD a row; matching
    /// prefixes REPLACE. Returns `Ok(self)` even when the file is
    /// missing — that's a "user didn't set it" not an error.
    pub fn with_override_path(mut self, path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(self);
        }
        let body =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let parsed = parse_override_toml(&body)?;
        for row in parsed {
            self.upsert(row);
        }
        // Re-sort after the upsert so the longest-prefix order is
        // preserved.
        self.rows.sort_by_key(|r| std::cmp::Reverse(r.prefix.len()));
        Ok(self)
    }

    fn upsert(&mut self, row: PricingRow) {
        if let Some(existing) = self.rows.iter_mut().find(|r| r.prefix == row.prefix) {
            existing.input_cents_per_million = row.input_cents_per_million;
            existing.output_cents_per_million = row.output_cents_per_million;
        } else {
            self.rows.push(row);
        }
    }

    /// Lookup the input/output rate for `model`. Falls back to the
    /// conservative frontier-class rate `(1500, 7500)` for unknown
    /// ids so the cost dashboard *over-* rather than *under-*
    /// estimates.
    pub fn rate_for(&self, model: &str) -> (u64, u64) {
        for row in &self.rows {
            if model.starts_with(&row.prefix) {
                return (row.input_cents_per_million, row.output_cents_per_million);
            }
        }
        (1500, 7500)
    }

    /// Sorted iterator over the table — exposed so docs/tests can
    /// print the shipped snapshot.
    pub fn rows(&self) -> &[PricingRow] {
        &self.rows
    }
}

/// Convenience: compute cost in integer cents for one `(model,
/// prompt_tokens, completion_tokens)` triple. Pass `Some(table)` to
/// use a pre-loaded override; `None` falls back to a fresh
/// `PricingTable::baked_in()` overlaid with `MTY_PRICING_OVERRIDE`.
pub fn cost_cents_for(
    model: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    table: Option<&PricingTable>,
) -> i64 {
    let owned;
    let table = match table {
        Some(t) => t,
        None => {
            owned = load_pricing_overrides();
            &owned
        }
    };
    let (in_rate, out_rate) = table.rate_for(model);
    let cost = prompt_tokens.saturating_mul(in_rate) / 1_000_000
        + completion_tokens.saturating_mul(out_rate) / 1_000_000;
    cost as i64
}

/// Load `PricingTable::baked_in()` overlaid with whatever
/// `MTY_PRICING_OVERRIDE` points at (if set). Errors in the override
/// file fall back to the baked-in table — observability must never
/// break the user's program.
pub fn load_pricing_overrides() -> PricingTable {
    let base = PricingTable::baked_in();
    let path = match std::env::var("MTY_PRICING_OVERRIDE") {
        Ok(p) if !p.is_empty() => p,
        _ => return base,
    };
    base.with_override_path(&path).unwrap_or_else(|e| {
        eprintln!("mty observe: ignoring MTY_PRICING_OVERRIDE ({e})");
        PricingTable::baked_in()
    })
}

/// Tiny hand-rolled TOML parser tailored to the override file shape.
/// We don't pull a full `toml` crate dep for a five-key schema; the
/// rest of the workspace has been carefully avoiding it. Supports:
///
/// ```toml
/// [[model]]
/// prefix = "claude-opus"
/// input_cents_per_million = 1500
/// output_cents_per_million = 7500
/// ```
///
/// Lines starting with `#` are comments. Blank lines OK. Anything
/// else is a parse error.
fn parse_override_toml(body: &str) -> Result<Vec<PricingRow>, String> {
    let mut out = Vec::new();
    let mut current: Option<PartialRow> = None;
    for (lineno, raw) in body.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[model]]" {
            if let Some(p) = current.take() {
                out.push(p.finish(lineno)?);
            }
            current = Some(PartialRow::default());
            continue;
        }
        let p = current.as_mut().ok_or_else(|| {
            format!(
                "line {}: key {line:?} appeared before any `[[model]]`",
                lineno + 1
            )
        })?;
        let (k, v) = line
            .split_once('=')
            .ok_or_else(|| format!("line {}: expected `key = value`", lineno + 1))?;
        let k = k.trim();
        let v = v.trim().trim_matches('"');
        match k {
            "prefix" => p.prefix = Some(v.to_string()),
            "input_cents_per_million" => {
                p.input = Some(
                    v.parse()
                        .map_err(|e| format!("line {}: input_cents parse: {e}", lineno + 1))?,
                )
            }
            "output_cents_per_million" => {
                p.output = Some(
                    v.parse()
                        .map_err(|e| format!("line {}: output_cents parse: {e}", lineno + 1))?,
                )
            }
            other => return Err(format!("line {}: unknown key `{other}`", lineno + 1)),
        }
    }
    if let Some(p) = current.take() {
        out.push(p.finish(body.lines().count())?);
    }
    Ok(out)
}

#[derive(Default)]
struct PartialRow {
    prefix: Option<String>,
    input: Option<u64>,
    output: Option<u64>,
}

impl PartialRow {
    fn finish(self, near_line: usize) -> Result<PricingRow, String> {
        Ok(PricingRow {
            prefix: self
                .prefix
                .ok_or_else(|| format!("line ~{near_line}: missing `prefix`"))?,
            input_cents_per_million: self
                .input
                .ok_or_else(|| format!("line ~{near_line}: missing `input_cents_per_million`"))?,
            output_cents_per_million: self
                .output
                .ok_or_else(|| format!("line ~{near_line}: missing `output_cents_per_million`"))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baked_in_has_all_four_provider_families() {
        let t = PricingTable::baked_in();
        assert_eq!(t.rate_for("claude-opus-4-7"), (1500, 7500));
        assert_eq!(t.rate_for("claude-sonnet-4-6"), (300, 1500));
        assert_eq!(t.rate_for("claude-haiku-4-5"), (100, 500));
        assert_eq!(t.rate_for("gpt-5"), (500, 1500));
        assert_eq!(t.rate_for("gemini-2.5-pro"), (125, 500));
        // Bedrock-hosted Anthropic
        assert_eq!(t.rate_for("anthropic.claude-opus-4-7-v1:0"), (1500, 7500));
    }

    #[test]
    fn longest_prefix_wins() {
        let t = PricingTable::baked_in();
        // gpt-4o-mini must win over gpt-4o.
        assert_eq!(t.rate_for("gpt-4o-mini"), (15, 60));
        assert_eq!(t.rate_for("gpt-4o-2026-05"), (250, 1000));
    }

    #[test]
    fn unknown_model_falls_back_to_conservative_default() {
        let t = PricingTable::baked_in();
        assert_eq!(t.rate_for("some-future-model"), (1500, 7500));
    }

    #[test]
    fn cost_for_known_model_computes_integer_cents() {
        // 1M opus input tokens = $15 = 1500 cents
        assert_eq!(cost_cents_for("claude-opus-4-7", 1_000_000, 0, None), 1500);
        // 100k opus output tokens = $7.50 = 750 cents
        assert_eq!(cost_cents_for("claude-opus-4-7", 0, 100_000, None), 750);
        // 1M sonnet output = 1500 cents
        assert_eq!(
            cost_cents_for("claude-sonnet-4-6", 0, 1_000_000, None),
            1500
        );
    }

    #[test]
    fn cost_for_unknown_model_uses_conservative_rate() {
        // 1M tokens at conservative (1500, 7500) = 1500 in cents on input.
        assert_eq!(
            cost_cents_for("some-future-model", 1_000_000, 0, None),
            1500
        );
    }

    #[test]
    fn parse_override_toml_basic() {
        let body = r#"
            # comment
            [[model]]
            prefix = "claude-opus"
            input_cents_per_million = 1000
            output_cents_per_million = 5000

            [[model]]
            prefix = "gpt-5"
            input_cents_per_million = 200
            output_cents_per_million = 800
        "#;
        let rows = parse_override_toml(body).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].prefix, "claude-opus");
        assert_eq!(rows[0].input_cents_per_million, 1000);
        assert_eq!(rows[1].prefix, "gpt-5");
    }

    #[test]
    fn parse_override_toml_rejects_malformed() {
        let body = "key_without_section = 1";
        assert!(parse_override_toml(body).is_err());
    }

    #[test]
    fn override_path_replaces_baked_in_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pricing.toml");
        std::fs::write(
            &path,
            r#"
            [[model]]
            prefix = "claude-opus"
            input_cents_per_million = 999
            output_cents_per_million = 999
            "#,
        )
        .unwrap();
        let t = PricingTable::baked_in().with_override_path(&path).unwrap();
        assert_eq!(t.rate_for("claude-opus-4-7"), (999, 999));
    }

    #[test]
    fn override_path_adds_new_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pricing.toml");
        std::fs::write(
            &path,
            r#"
            [[model]]
            prefix = "future-model"
            input_cents_per_million = 10
            output_cents_per_million = 20
            "#,
        )
        .unwrap();
        let t = PricingTable::baked_in().with_override_path(&path).unwrap();
        assert_eq!(t.rate_for("future-model-v2"), (10, 20));
    }

    #[test]
    fn override_missing_file_returns_baked_in_unchanged() {
        let t = PricingTable::baked_in()
            .with_override_path("/nonexistent/path/x.toml")
            .unwrap();
        // baked-in opus rate is untouched
        assert_eq!(t.rate_for("claude-opus-4-7"), (1500, 7500));
    }
}
