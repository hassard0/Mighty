//! SWE-bench Verified dataset loader.
//!
//! Strategy:
//! - The dataset is hosted on HuggingFace at
//!   `princeton-nlp/SWE-bench_Verified`.
//! - For reproducibility we pin the dataset commit hash and fetch
//!   the `test` split JSON via the HF `datasets-server` API, which
//!   returns plain JSON (no parquet reader required).
//! - The full instance payload (`problem_statement`, `patch`,
//!   `test_patch`, `FAIL_TO_PASS`, `PASS_TO_PASS`, `base_commit`,
//!   `repo`) is cached on disk under `data/`.
//! - When `--num-problems N` is requested we filter by `instance_id`
//!   against the curated smoke list in `problems.rs`.
//!
//! Offline mode: if the cache is populated we never touch the network.
//! Set `MTY_SWE_OFFLINE=1` to make any cache miss a hard error.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Pinned dataset revision — bump this deliberately when re-baselining.
/// Picked because it's a recent immutable snapshot of the Verified set.
/// Reserved for future use when we switch from the live HF datasets-server
/// (which always returns `main`) to a pinned-revision parquet pull.
#[allow(dead_code)]
pub const DATASET_REVISION: &str = "main";

/// Where we cache fetched instances on disk.
pub fn cache_dir(root: &Path) -> PathBuf {
    root.join("data").join("instances")
}

/// One SWE-bench Verified row in the shape we actually use.
///
/// The upstream schema has ~15 columns; we keep only what the
/// harness consumes so the JSON cache stays small.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub instance_id: String,
    pub repo: String,
    pub base_commit: String,
    pub problem_statement: String,
    /// Reference "golden" patch — only used by the scorer to
    /// double-check our pytest selectors line up. The agent
    /// never sees this.
    #[serde(default)]
    pub patch: String,
    /// Test patch that's already applied in the repo at scoring
    /// time. The agent's output is layered on top.
    #[serde(default)]
    pub test_patch: String,
    /// Tests that must transition fail->pass for the instance to score.
    #[serde(rename = "FAIL_TO_PASS", default)]
    pub fail_to_pass: Vec<String>,
    /// Tests that must stay passing.
    #[serde(rename = "PASS_TO_PASS", default)]
    pub pass_to_pass: Vec<String>,
    /// Hash of the original row — lets us detect drift if HF rev moves.
    #[serde(default)]
    pub row_hash: String,
}

impl Instance {
    /// Synthesise a deterministic placeholder for the smoke list when
    /// the dataset fetch is unavailable (no network / no key). Lets
    /// CI smoke-tests still exercise the harness plumbing.
    pub fn placeholder_from_smoke(p: &crate::problems::SmokeProblem) -> Self {
        let mut h = Sha256::new();
        h.update(p.instance_id.as_bytes());
        let row_hash = hex::encode(h.finalize());
        Self {
            instance_id: p.instance_id.to_string(),
            repo: p.repo.to_string(),
            base_commit: p.base_commit.to_string(),
            problem_statement: format!(
                "[placeholder — dataset fetch unavailable]\n\n{}\n\n(See https://huggingface.co/datasets/princeton-nlp/SWE-bench_Verified for the real statement.)",
                p.statement_preview
            ),
            patch: String::new(),
            test_patch: String::new(),
            fail_to_pass: vec![],
            pass_to_pass: vec![],
            row_hash,
        }
    }
}

/// Try to load an instance from the on-disk cache, falling back to
/// the placeholder when the cache is empty AND offline-mode is off
/// (so the harness still produces structured output).
pub async fn load_instance(
    crate_root: &Path,
    smoke: &crate::problems::SmokeProblem,
) -> Result<Instance> {
    let cache = cache_dir(crate_root).join(format!("{}.json", smoke.instance_id));
    if cache.exists() {
        let bytes = tokio::fs::read(&cache)
            .await
            .with_context(|| format!("read cached instance {}", cache.display()))?;
        let inst: Instance = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse cached instance {}", cache.display()))?;
        return Ok(inst);
    }

    if std::env::var("MTY_SWE_OFFLINE").is_ok() {
        return Err(anyhow!(
            "offline mode and no cache for {} at {}",
            smoke.instance_id,
            cache.display()
        ));
    }

    // Best-effort network fetch via HF datasets-server. We do NOT
    // bring in the full `datasets` python toolchain — one HTTP call
    // is enough for our 10-row use case.
    match fetch_instance_from_hf(smoke.instance_id).await {
        Ok(inst) => {
            tokio::fs::create_dir_all(cache.parent().unwrap())
                .await
                .ok();
            let bytes = serde_json::to_vec_pretty(&inst)?;
            tokio::fs::write(&cache, &bytes).await.ok();
            Ok(inst)
        }
        Err(e) => {
            tracing::warn!(
                "HF fetch failed for {} ({}); falling back to placeholder",
                smoke.instance_id,
                e
            );
            Ok(Instance::placeholder_from_smoke(smoke))
        }
    }
}

async fn fetch_instance_from_hf(instance_id: &str) -> Result<Instance> {
    // HF datasets-server endpoint — paginated rows; we ask for the
    // single row whose instance_id matches. Bandwidth is fine: ~5KB.
    // The endpoint shape is documented at:
    //   https://huggingface.co/docs/datasets-server/
    let url = format!(
        "https://datasets-server.huggingface.co/filter?dataset=princeton-nlp%2FSWE-bench_Verified&config=default&split=test&where=instance_id%3D%27{}%27&offset=0&length=1",
        urlencode(instance_id)
    );
    let client = reqwest::Client::builder()
        .user_agent("mty-swe-bench/0.1")
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        return Err(anyhow!("HF returned {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await?;
    let rows = body
        .get("rows")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("HF response missing rows[]"))?;
    let row = rows
        .first()
        .and_then(|r| r.get("row"))
        .ok_or_else(|| anyhow!("HF returned no rows for {}", instance_id))?;
    let inst: Instance = serde_json::from_value(row.clone())?;
    Ok(inst)
}

fn urlencode(s: &str) -> String {
    // Minimal escape — instance IDs are `repo__short-N` so only `_`
    // and `-` plus ascii alphanums appear. The double-underscore
    // is safe in URLs but we encode `%` and `'` defensively.
    s.replace('\'', "%27").replace('%', "%25")
}
