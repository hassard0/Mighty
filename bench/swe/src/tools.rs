//! Capability-typed tool surface the Mighty agent exposes to the LLM.
//!
//! Each `Tool` has:
//!   * A JSON Schema describing its inputs (sent to Claude on every turn).
//!   * A capability declaration narrowing what the tool can do (e.g.
//!     `fs.read(<repo>)` confines reads to the workspace).
//!   * A Rust `execute()` impl the harness calls when Claude emits
//!     a `tool_use` block.
//!
//! In the Mighty source spec (`agent.mty`) these are declared with
//! the `@tool("desc", cap: fs.read)` attribute. Here in the Rust
//! harness we mirror the same shape so the `.mty` file can be
//! parser-checked separately while the smoke runner actually drives
//! the API.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::llm::Tool;

/// Workspace the tools are confined to. Every `fs.*` and `cmd.*` cap
/// is rooted here.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
}

impl Workspace {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn safe_join(&self, rel: &str) -> Result<PathBuf> {
        // Reject absolute paths + `..` traversal — this enforces
        // the `fs.read(<repo>)` capability narrowing.
        //
        // We treat any of these as absolute (covers both POSIX and
        // Windows shapes — `Path::is_absolute` is platform-dependent
        // and would let `/etc/passwd` through on Windows):
        //   * leading `/` or `\`
        //   * `<letter>:` drive prefix (`C:\…`, `c:/…`)
        //   * UNC (`\\server\share\…`)
        if rel.starts_with('/') || rel.starts_with('\\') {
            bail!("absolute paths not allowed: {rel}");
        }
        if rel.len() >= 2 && rel.as_bytes()[1] == b':' {
            bail!("absolute drive paths not allowed: {rel}");
        }
        if Path::new(rel).is_absolute() {
            bail!("absolute paths not allowed: {rel}");
        }
        // Both `/` and `\` count as separators for traversal checks.
        if rel.split(['/', '\\']).any(|c| c == "..") {
            bail!("`..` traversal not allowed: {rel}");
        }
        Ok(self.root.join(rel))
    }
}

/// The Anthropic-shape `Tool` list the harness sends each turn.
pub fn tool_specs() -> Vec<Tool> {
    vec![
        Tool {
            name: "read_file".into(),
            description: "Read the contents of a file relative to the repo root. \
                Capability: fs.read(<repo>)."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Repo-relative path"},
                },
                "required": ["path"],
            }),
        },
        Tool {
            name: "write_file".into(),
            description: "Overwrite the contents of a file relative to the repo root. \
                Capability: fs.write(<repo>). Creates parent directories if needed."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                },
                "required": ["path", "content"],
            }),
        },
        Tool {
            name: "list_dir".into(),
            description: "List files in a directory relative to the repo root. \
                Capability: fs.read(<repo>)."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Repo-relative; '.' for root"},
                },
                "required": ["path"],
            }),
        },
        Tool {
            name: "apply_patch".into(),
            description: "Apply a unified-diff patch to the repo. \
                Capability: fs.write(<repo>). Equivalent to `git apply <patch>`."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "patch": {"type": "string", "description": "Unified diff text"},
                },
                "required": ["patch"],
            }),
        },
        Tool {
            name: "run_tests".into(),
            description: "Run pytest with the given selector and return its output. \
                Capability: cmd.exec(\"pytest\"). Time-capped at 90s."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "selector": {"type": "string", "description": "pytest -k expression or path"},
                },
                "required": ["selector"],
            }),
        },
        Tool {
            name: "submit".into(),
            description: "Signal that the agent believes the patch is complete \
                and the harness should run the failing-test selector to score."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "summary": {"type": "string"},
                },
                "required": ["summary"],
            }),
        },
    ]
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReadFileArgs {
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WriteFileArgs {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListDirArgs {
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApplyPatchArgs {
    pub patch: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunTestsArgs {
    pub selector: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitArgs {
    pub summary: String,
}

pub async fn exec_read_file(ws: &Workspace, args: ReadFileArgs) -> Result<String> {
    let p = ws.safe_join(&args.path)?;
    let bytes = tokio::fs::read(&p)
        .await
        .with_context(|| format!("read_file {}", p.display()))?;
    // Truncate to 64KB — Claude doesn't need megabytes per turn, and
    // it keeps token costs sane.
    const MAX: usize = 64 * 1024;
    if bytes.len() > MAX {
        let head = String::from_utf8_lossy(&bytes[..MAX]).to_string();
        Ok(format!(
            "{head}\n\n[... truncated, file is {} bytes]",
            bytes.len()
        ))
    } else {
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }
}

pub async fn exec_write_file(ws: &Workspace, args: WriteFileArgs) -> Result<String> {
    let p = ws.safe_join(&args.path)?;
    if let Some(parent) = p.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::write(&p, args.content.as_bytes())
        .await
        .with_context(|| format!("write_file {}", p.display()))?;
    Ok(format!(
        "wrote {} bytes to {}",
        args.content.len(),
        args.path
    ))
}

pub async fn exec_list_dir(ws: &Workspace, args: ListDirArgs) -> Result<String> {
    let p = ws.safe_join(&args.path)?;
    let mut rd = tokio::fs::read_dir(&p)
        .await
        .with_context(|| format!("list_dir {}", p.display()))?;
    let mut entries = Vec::new();
    while let Some(e) = rd.next_entry().await? {
        let name = e.file_name().to_string_lossy().to_string();
        let kind = if e.file_type().await?.is_dir() {
            "dir"
        } else {
            "file"
        };
        entries.push(format!("{kind}\t{name}"));
    }
    entries.sort();
    Ok(entries.join("\n"))
}

pub async fn exec_apply_patch(ws: &Workspace, args: ApplyPatchArgs) -> Result<String> {
    // Write the patch to a temp file inside the workspace so
    // `git apply` can find it.
    let patch_path = ws.root.join(".mty-patch.diff");
    tokio::fs::write(&patch_path, args.patch.as_bytes()).await?;
    let out = tokio::process::Command::new("git")
        .arg("apply")
        .arg("--whitespace=nowarn")
        .arg(".mty-patch.diff")
        .current_dir(&ws.root)
        .output()
        .await
        .context("spawn git apply")?;
    let _ = tokio::fs::remove_file(&patch_path).await;
    if !out.status.success() {
        return Err(anyhow!(
            "git apply failed: {}\nstderr:\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(format!(
        "patch applied:\n{}",
        String::from_utf8_lossy(&out.stdout)
    ))
}

pub async fn exec_run_tests(ws: &Workspace, args: RunTestsArgs) -> Result<String> {
    // Bound test time at 90s — keeps a runaway test suite from
    // eating the run budget.
    let fut = tokio::process::Command::new("python")
        .arg("-m")
        .arg("pytest")
        .arg("-x")
        .arg("--tb=short")
        .arg(&args.selector)
        .current_dir(&ws.root)
        .output();
    let out = match tokio::time::timeout(std::time::Duration::from_secs(90), fut).await {
        Ok(r) => r.context("spawn pytest")?,
        Err(_) => return Ok("[TIMEOUT after 90s]".into()),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    Ok(format!(
        "exit: {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        out.status.code().unwrap_or(-1),
        truncate(&stdout, 8 * 1024),
        truncate(&stderr, 2 * 1024)
    ))
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}\n\n[... truncated, {} more bytes]", &s[..n], s.len() - n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_rejects_absolute() {
        let ws = Workspace::new("/tmp/repo");
        // POSIX-style absolute
        assert!(ws.safe_join("/etc/passwd").is_err());
        // Windows drive-letter forms
        assert!(ws.safe_join("C:\\Windows\\system32").is_err());
        assert!(ws.safe_join("c:/Windows/system32").is_err());
        // Backslash-only absolute
        assert!(ws.safe_join("\\Windows\\system32").is_err());
    }

    #[test]
    fn safe_join_rejects_traversal() {
        let ws = Workspace::new("/tmp/repo");
        assert!(ws.safe_join("../../etc/passwd").is_err());
        assert!(ws.safe_join("src/../../etc").is_err());
        // Backslash separators on Windows-style paths must also count.
        assert!(ws.safe_join("src\\..\\..\\etc").is_err());
    }

    #[test]
    fn safe_join_allows_relative() {
        let ws = Workspace::new("/tmp/repo");
        assert!(ws.safe_join("src/main.rs").is_ok());
        assert!(ws.safe_join("./tests/x.py").is_ok());
    }

    #[test]
    fn tool_specs_has_expected_set() {
        let names: Vec<_> = tool_specs().into_iter().map(|t| t.name).collect();
        for expected in [
            "read_file",
            "write_file",
            "list_dir",
            "apply_patch",
            "run_tests",
            "submit",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "missing tool {expected}"
            );
        }
    }
}
