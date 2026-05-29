//! v0.34 T4 — `mty hooks install`: install the project's pre-push
//! hook (`.git-hooks/pre-push`) into `.git/hooks/pre-push`.
//!
//! The hook itself mirrors the two cheapest CI gates: `cargo fmt
//! --all -- --check` and `cargo clippy --workspace --all-targets --
//! -D warnings`. Running them locally on every `git push` catches the
//! recurring v0.27/v0.30/v0.32/v0.33 Linux/Windows fmt-drift class of
//! regressions before they hit CI. See
//! [`docs/contributing.md`](../../../../docs/contributing.md) — the
//! hook is documented as REQUIRED for swarm-agent setups.
//!
//! ## Mechanics
//!
//! 1. Locate the repo root by walking up from cwd looking for `.git`.
//!    Worktrees (where `.git` is a file pointing at the main repo's
//!    `worktrees/<name>` directory) are handled transparently: the
//!    hook lands in the worktree's own hook directory, not the main
//!    repo's.
//! 2. Read the existing `.git/hooks/pre-push` (if any) and refuse to
//!    overwrite anything we didn't author ourselves (header sentinel).
//!    `--force` bypasses the check.
//! 3. Copy `.git-hooks/pre-push` into place. On Unix, also `chmod
//!    +x`. On Windows the file-system has no execute bit; git for
//!    Windows runs `*.sample`-style files via mingw bash which doesn't
//!    require the bit anyway.
//!
//! ## Why a copy and not a symlink
//!
//! Symlinks on Windows require either Developer Mode or admin rights;
//! we want this command to "just work" for every contributor, so the
//! copy-with-sentinel pattern is more robust than the symlink-with-
//! fallback dance.

use std::path::{Path, PathBuf};

/// Sentinel line in the hook script that identifies it as ours.
/// Refused-overwrite logic checks for this in any pre-existing hook.
const HOOK_SENTINEL: &str = "Mighty pre-push hook — v0.34 T4.";

/// Public entry point used by `main.rs`. Returns the process exit
/// code (0 = success).
pub fn run(action: HooksAction) -> i32 {
    match action {
        HooksAction::Install { force } => install(force),
        HooksAction::Uninstall => uninstall(),
        HooksAction::Status => status(),
    }
}

/// CLI surface mirrored from `Cmd::Hooks` in `main.rs`.
#[derive(Debug, Clone)]
pub enum HooksAction {
    /// Copy `.git-hooks/pre-push` into `.git/hooks/pre-push`. With
    /// `force`, overwrite any pre-existing hook even if we didn't
    /// author it.
    Install { force: bool },
    /// Remove the installed pre-push hook if it carries our sentinel.
    Uninstall,
    /// Print whether the hook is installed.
    Status,
}

fn install(force: bool) -> i32 {
    let Some(root) = find_repo_root() else {
        eprintln!(
            "mty hooks install: no repo root found from {}",
            std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "<cwd>".to_string())
        );
        return 2;
    };
    let source = root.join(".git-hooks").join("pre-push");
    let hooks_dir = match resolve_hooks_dir(&root) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("mty hooks install: {e}");
            return 2;
        }
    };
    let target = hooks_dir.join("pre-push");

    if !source.exists() {
        eprintln!(
            "mty hooks install: source `{}` missing — is this the Mighty repo?",
            source.display()
        );
        return 2;
    }

    // Refuse to overwrite a non-mty hook unless forced.
    if target.exists() && !force {
        let existing = std::fs::read_to_string(&target).unwrap_or_default();
        if !existing.contains(HOOK_SENTINEL) {
            eprintln!(
                "mty hooks install: {} already exists and is not a Mighty hook.\n  Pass --force to overwrite, or remove the existing hook by hand.",
                target.display(),
            );
            return 1;
        }
    }

    if let Some(parent) = target.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!(
                "mty hooks install: failed to create {}: {e}",
                parent.display()
            );
            return 2;
        }
    }

    if let Err(e) = std::fs::copy(&source, &target) {
        eprintln!(
            "mty hooks install: copy {} -> {} failed: {e}",
            source.display(),
            target.display()
        );
        return 2;
    }

    // Unix-only: chmod +x. Windows hosts ignore the executable bit;
    // git-for-windows runs hooks via the included bash regardless.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&target) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&target, perms);
        }
    }

    println!(
        "mty hooks install: installed pre-push hook at {}",
        target.display()
    );
    0
}

fn uninstall() -> i32 {
    let Some(root) = find_repo_root() else {
        eprintln!("mty hooks uninstall: no repo root found");
        return 2;
    };
    let hooks_dir = match resolve_hooks_dir(&root) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("mty hooks uninstall: {e}");
            return 2;
        }
    };
    let target = hooks_dir.join("pre-push");
    if !target.exists() {
        println!("mty hooks uninstall: no pre-push hook installed");
        return 0;
    }
    let existing = std::fs::read_to_string(&target).unwrap_or_default();
    if !existing.contains(HOOK_SENTINEL) {
        eprintln!(
            "mty hooks uninstall: {} is not a Mighty hook — leaving it alone",
            target.display()
        );
        return 1;
    }
    if let Err(e) = std::fs::remove_file(&target) {
        eprintln!(
            "mty hooks uninstall: failed to remove {}: {e}",
            target.display()
        );
        return 2;
    }
    println!("mty hooks uninstall: removed {}", target.display());
    0
}

fn status() -> i32 {
    let Some(root) = find_repo_root() else {
        eprintln!("mty hooks status: no repo root found");
        return 2;
    };
    let hooks_dir = match resolve_hooks_dir(&root) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("mty hooks status: {e}");
            return 2;
        }
    };
    let target = hooks_dir.join("pre-push");
    if !target.exists() {
        println!("mty hooks status: not installed");
        return 0;
    }
    let existing = std::fs::read_to_string(&target).unwrap_or_default();
    if existing.contains(HOOK_SENTINEL) {
        println!(
            "mty hooks status: Mighty pre-push hook installed at {}",
            target.display()
        );
    } else {
        println!(
            "mty hooks status: {} exists but is not a Mighty hook",
            target.display()
        );
    }
    0
}

/// Find the repo root by walking up from cwd looking for a `.git`
/// directory or file. Returns the directory containing `.git`.
pub fn find_repo_root() -> Option<PathBuf> {
    let mut cur = std::env::current_dir().ok()?;
    loop {
        if cur.join(".git").exists() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Given a repo root, resolve the directory hooks should land in.
///
/// Worktrees: `.git` is a file containing `gitdir: <path>`. The hooks
/// directory in a worktree is `<gitdir>/hooks`, so we honour that
/// rather than landing the hook in the main repo's hook dir.
///
/// Regular checkouts: `.git/hooks`.
pub fn resolve_hooks_dir(root: &Path) -> Result<PathBuf, String> {
    let dotgit = root.join(".git");
    let meta = std::fs::metadata(&dotgit)
        .map_err(|e| format!("cannot stat `{}`: {e}", dotgit.display()))?;
    if meta.is_dir() {
        return Ok(dotgit.join("hooks"));
    }
    if meta.is_file() {
        // Worktree pointer file. Format: `gitdir: <abs or relative path>\n`.
        let txt = std::fs::read_to_string(&dotgit)
            .map_err(|e| format!("cannot read `{}`: {e}", dotgit.display()))?;
        let line = txt
            .lines()
            .find(|l| l.starts_with("gitdir:"))
            .ok_or_else(|| format!("`{}` has no `gitdir:` line", dotgit.display()))?;
        let gitdir_raw = line.trim_start_matches("gitdir:").trim();
        let gitdir = PathBuf::from(gitdir_raw);
        let gitdir = if gitdir.is_absolute() {
            gitdir
        } else {
            root.join(gitdir)
        };
        return Ok(gitdir.join("hooks"));
    }
    Err(format!("`{}` is not a directory or file", dotgit.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_is_present_in_source_hook_when_run_from_repo() {
        // Best-effort: if we can find the repo root from the test
        // process's cwd, the source hook must carry the sentinel.
        if let Some(root) = find_repo_root() {
            let source = root.join(".git-hooks").join("pre-push");
            if source.exists() {
                let body = std::fs::read_to_string(&source).unwrap();
                assert!(
                    body.contains(HOOK_SENTINEL),
                    "source hook must contain sentinel `{HOOK_SENTINEL}`"
                );
            }
        }
    }

    #[test]
    fn resolve_hooks_dir_for_plain_repo() {
        // Smoke: in a real worktree-or-checkout cwd, resolve_hooks_dir
        // returns a path ending in `hooks`.
        if let Some(root) = find_repo_root() {
            if let Ok(p) = resolve_hooks_dir(&root) {
                assert!(p.ends_with("hooks"), "expected …/hooks, got {p:?}");
            }
        }
    }

    #[test]
    fn sentinel_constant_stable() {
        // Pin the sentinel string. Changing this requires bumping
        // anyone who installed an older hook (they'd be considered
        // "not a Mighty hook" by the new compiler and have to
        // re-install with --force).
        assert_eq!(HOOK_SENTINEL, "Mighty pre-push hook — v0.34 T4.");
    }
}
