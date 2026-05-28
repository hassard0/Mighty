//! Per-instance ephemeral git workspace.
//!
//! For each problem we:
//!   1. Shallow-clone the upstream repo into a temp dir.
//!   2. `git checkout <base_commit>`.
//!   3. Hand the workspace path to the agent.
//!   4. After the agent submits, diff the workspace against
//!      `base_commit` to extract the produced patch.
//!   5. Re-checkout the base commit, apply the agent's patch +
//!      the dataset's `test_patch`, run the failing-test selector.
//!
//! The git clone is the only network operation per instance. If it
//! fails (rate-limited, repo moved, etc.) the harness records
//! `Skipped { reason }` and moves on.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use tokio::process::Command;

pub struct GitWorkspace {
    pub root: PathBuf,
    /// Repo slug — kept for trace logs + JSON report cross-checking.
    #[allow(dead_code)]
    pub repo: String,
    pub base_commit: String,
}

impl GitWorkspace {
    /// Clone `https://github.com/<repo>.git` at `base_commit` into a
    /// fresh subdirectory of `parent_dir`.
    pub async fn clone_at(parent_dir: &Path, repo: &str, base_commit: &str) -> Result<Self> {
        let safe = repo.replace('/', "__");
        let root = parent_dir.join(format!(
            "{safe}__{}",
            &base_commit[..8.min(base_commit.len())]
        ));
        if root.exists() {
            // Re-use an existing checkout if one is already there
            // (lets `bench-smoke` be re-runnable without re-cloning).
            return Ok(Self {
                root,
                repo: repo.to_string(),
                base_commit: base_commit.to_string(),
            });
        }
        tokio::fs::create_dir_all(parent_dir).await.ok();
        let url = format!("https://github.com/{repo}.git");
        // Full clone (not shallow) — shallow clones can't `git checkout
        // <sha>` to commits not in the shallow window.
        let out = Command::new("git")
            .arg("clone")
            .arg("--quiet")
            .arg(&url)
            .arg(&root)
            .output()
            .await
            .context("spawn git clone")?;
        if !out.status.success() {
            return Err(anyhow!(
                "git clone {} failed: {}",
                url,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        let out = Command::new("git")
            .arg("checkout")
            .arg("--quiet")
            .arg(base_commit)
            .current_dir(&root)
            .output()
            .await
            .context("spawn git checkout")?;
        if !out.status.success() {
            return Err(anyhow!(
                "git checkout {} failed: {}",
                base_commit,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(Self {
            root,
            repo: repo.to_string(),
            base_commit: base_commit.to_string(),
        })
    }

    /// Reset the workspace back to the base commit (drop the agent's edits).
    pub async fn reset(&self) -> Result<()> {
        let out = Command::new("git")
            .arg("reset")
            .arg("--hard")
            .arg("--quiet")
            .arg(&self.base_commit)
            .current_dir(&self.root)
            .output()
            .await?;
        if !out.status.success() {
            return Err(anyhow!(
                "git reset --hard {}: {}",
                self.base_commit,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        let _ = Command::new("git")
            .arg("clean")
            .arg("-fd")
            .arg("--quiet")
            .current_dir(&self.root)
            .output()
            .await;
        Ok(())
    }

    /// Diff the workspace against the base commit. Returns the patch text.
    pub async fn diff(&self) -> Result<String> {
        let out = Command::new("git")
            .arg("diff")
            .arg(&self.base_commit)
            .current_dir(&self.root)
            .output()
            .await?;
        if !out.status.success() {
            return Err(anyhow!("git diff failed"));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}
