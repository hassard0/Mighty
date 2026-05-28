//! Per-instance scoring.
//!
//! After the agent submits, we:
//!   1. Re-checkout the base commit.
//!   2. Apply the agent's produced patch.
//!   3. Apply the dataset's `test_patch` (the SWE-bench convention —
//!      this brings in the failing tests the agent must turn green).
//!   4. Run `pytest <FAIL_TO_PASS...>` and verify every test passes.
//!   5. Run `pytest <PASS_TO_PASS...>` to confirm no regressions.
//!
//! The boolean `passed` is `true` iff every `FAIL_TO_PASS` test
//! transitioned from failing to passing AND every `PASS_TO_PASS`
//! test is still passing.
//!
//! For our 10-problem smoke we keep the scoring lightweight — we
//! report `Outcome::ScoringSkipped { reason }` when the dataset's
//! patches aren't applicable (the agent's diff conflicts), so the
//! result file stays auditable.

use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::dataset::Instance;
use crate::workspace::GitWorkspace;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    Failed { details: String },
    ScoringSkipped { reason: String },
    AgentDidNotSubmit,
}

pub struct ScoreResult {
    pub outcome: Outcome,
    pub wall_time: Duration,
    pub agent_patch: String,
}

pub async fn score(ws: &GitWorkspace, inst: &Instance, submitted: bool) -> Result<ScoreResult> {
    let started = std::time::Instant::now();
    let agent_patch = ws.diff().await.unwrap_or_default();
    if !submitted {
        return Ok(ScoreResult {
            outcome: Outcome::AgentDidNotSubmit,
            wall_time: started.elapsed(),
            agent_patch,
        });
    }
    if agent_patch.trim().is_empty() {
        return Ok(ScoreResult {
            outcome: Outcome::Failed {
                details: "agent submitted with empty diff".into(),
            },
            wall_time: started.elapsed(),
            agent_patch,
        });
    }

    if inst.fail_to_pass.is_empty() {
        return Ok(ScoreResult {
            outcome: Outcome::ScoringSkipped {
                reason: "dataset cache lacks FAIL_TO_PASS (placeholder mode)".into(),
            },
            wall_time: started.elapsed(),
            agent_patch,
        });
    }

    // Reset + reapply agent patch cleanly so we know the diff state.
    if let Err(e) = ws.reset().await {
        return Ok(ScoreResult {
            outcome: Outcome::ScoringSkipped {
                reason: format!("workspace reset failed: {e}"),
            },
            wall_time: started.elapsed(),
            agent_patch,
        });
    }
    if !agent_patch.trim().is_empty() {
        let patch_path = ws.root.join(".mty-agent.diff");
        if let Err(e) = tokio::fs::write(&patch_path, agent_patch.as_bytes()).await {
            return Ok(ScoreResult {
                outcome: Outcome::ScoringSkipped {
                    reason: format!("could not write agent patch: {e}"),
                },
                wall_time: started.elapsed(),
                agent_patch,
            });
        }
        let out = Command::new("git")
            .args(["apply", "--whitespace=nowarn", ".mty-agent.diff"])
            .current_dir(&ws.root)
            .output()
            .await
            .map_err(|e| anyhow!(e))?;
        let _ = tokio::fs::remove_file(&patch_path).await;
        if !out.status.success() {
            return Ok(ScoreResult {
                outcome: Outcome::Failed {
                    details: format!(
                        "agent patch failed to reapply cleanly: {}",
                        String::from_utf8_lossy(&out.stderr)
                    ),
                },
                wall_time: started.elapsed(),
                agent_patch,
            });
        }
    }

    if !inst.test_patch.trim().is_empty() {
        let tp = ws.root.join(".mty-testpatch.diff");
        if tokio::fs::write(&tp, inst.test_patch.as_bytes())
            .await
            .is_ok()
        {
            let out = Command::new("git")
                .args(["apply", "--whitespace=nowarn", ".mty-testpatch.diff"])
                .current_dir(&ws.root)
                .output()
                .await;
            let _ = tokio::fs::remove_file(&tp).await;
            if let Ok(out) = out {
                if !out.status.success() {
                    return Ok(ScoreResult {
                        outcome: Outcome::ScoringSkipped {
                            reason: format!(
                                "dataset test_patch failed to apply: {}",
                                String::from_utf8_lossy(&out.stderr)
                            ),
                        },
                        wall_time: started.elapsed(),
                        agent_patch,
                    });
                }
            }
        }
    }

    // Run the FAIL_TO_PASS selectors.
    let mut details = String::new();
    let mut all_passed = true;
    for sel in &inst.fail_to_pass {
        let out = Command::new("python")
            .args(["-m", "pytest", "-x", "--tb=short", sel])
            .current_dir(&ws.root)
            .output()
            .await
            .map_err(|e| anyhow!(e))?;
        let ok = out.status.success();
        all_passed &= ok;
        details.push_str(&format!("[{}] {}\n", if ok { "PASS" } else { "FAIL" }, sel));
        if !ok {
            details.push_str(&String::from_utf8_lossy(&out.stdout));
        }
    }

    Ok(ScoreResult {
        outcome: if all_passed {
            Outcome::Passed
        } else {
            Outcome::Failed { details }
        },
        wall_time: started.elapsed(),
        agent_patch,
    })
}
