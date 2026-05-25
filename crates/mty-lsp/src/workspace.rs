//! v0.8 loose-end 3/4 — cross-file workspace resolve map.
//!
//! Single-file analysis (the v0.5 baseline) is fine for hover and
//! per-file diagnostics, but rename / go-to-def / find-references
//! across files needs a workspace-wide index. This module builds one.
//!
//! ## Model
//!
//! A [`WorkspaceModel`] keyed by workspace root folder. Inside each
//! folder we walk for `.mty` files (capped at [`MAX_FILES`] so a
//! pathological scan stays bounded) and cache a [`crate::docs::DocAnalysis`]
//! per file plus a top-level-name index.
//!
//! Refresh triggers (driven by [`crate::server::Backend`]):
//!
//!   * `initialize`: any `workspaceFolders` are scanned.
//!   * `did_change_workspace_folders`: added folders are scanned, removed
//!     folders are dropped.
//!   * `did_change_watched_files`: file create/delete/rename invalidates
//!     and re-analyses the affected entries.
//!   * `did_change` for an OPEN file: we update the in-memory snapshot;
//!     subsequent cross-file queries see the unsaved buffer.

use crate::docs::DocAnalysis;
use crate::references::Occurrence;
use dashmap::DashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_lsp::lsp_types::Url;

/// Per-folder cap on the file-scan walker. Workspaces larger than this
/// will be partially indexed; cross-file rename on a name in an
/// un-indexed file falls back to single-file behaviour.
pub const MAX_FILES: usize = 2048;

/// One file's cached analysis inside the workspace model.
#[derive(Clone)]
pub struct WorkspaceFile {
    /// LSP `Url` corresponding to the file path.
    pub uri: Url,
    /// On-disk path. Used as the key inside [`WorkspaceModel`].
    pub path: PathBuf,
    /// Parsed + lowered + type-checked snapshot.
    pub analysis: Arc<DocAnalysis>,
}

/// One workspace folder. We track files keyed by absolute path so
/// rename can produce a deterministic ordering for the WorkspaceEdit.
#[derive(Default, Clone)]
pub struct WorkspaceModel {
    pub root: PathBuf,
    pub files: Arc<DashMap<PathBuf, WorkspaceFile>>,
}

impl WorkspaceModel {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            files: Arc::new(DashMap::new()),
        }
    }

    /// Walk `self.root` for `.mty` files up to [`MAX_FILES`]. Reads each
    /// file from disk and runs the per-doc analysis pipeline. Existing
    /// entries are overwritten (cheap because `DocAnalysis::analyze`
    /// is the bulk of the cost).
    pub fn rescan(&self) {
        let mut count = 0usize;
        let mut stack: Vec<PathBuf> = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            if count >= MAX_FILES {
                break;
            }
            let read = match std::fs::read_dir(&dir) {
                Ok(r) => r,
                Err(_) => continue,
            };
            for entry in read.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    if matches!(name, "target" | "node_modules" | ".git" | "build") {
                        continue;
                    }
                    stack.push(path);
                } else if path.extension().and_then(|s| s.to_str()) == Some("mty") {
                    self.analyze_path(&path);
                    count += 1;
                    if count >= MAX_FILES {
                        break;
                    }
                }
            }
        }
    }

    /// (Re-)analyze a single file from disk content. No-ops if the file
    /// can't be read.
    pub fn analyze_path(&self, path: &Path) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let uri = match path_to_uri(path) {
            Some(u) => u,
            None => return,
        };
        let analysis = Arc::new(DocAnalysis::analyze(text, uri.to_string(), 0));
        self.files.insert(
            path.to_path_buf(),
            WorkspaceFile {
                uri,
                path: path.to_path_buf(),
                analysis,
            },
        );
    }

    /// Apply an in-memory open-buffer update so cross-file queries
    /// reflect the user's unsaved edits.
    pub fn update_open(&self, path: PathBuf, uri: Url, analysis: Arc<DocAnalysis>) {
        self.files.insert(
            path.clone(),
            WorkspaceFile {
                uri,
                path,
                analysis,
            },
        );
    }

    /// Remove a file from the index (file deleted).
    pub fn drop_path(&self, path: &Path) {
        self.files.remove(path);
    }

    /// Find every reference to `name` across every file in the folder.
    /// Returns a list of `(file, occurrences-in-source-order)`.
    pub fn find_refs_across_files(&self, name: &str) -> Vec<(WorkspaceFile, Vec<Occurrence>)> {
        let mut out: Vec<(WorkspaceFile, Vec<Occurrence>)> = vec![];
        let mut entries: Vec<WorkspaceFile> =
            self.files.iter().map(|kv| kv.value().clone()).collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        for f in entries {
            let occs = crate::references::find_top_level_refs(&f.analysis, name);
            if !occs.is_empty() {
                out.push((f, occs));
            }
        }
        out
    }
}

/// Top-level workspace registry held by the LSP backend. Keyed by
/// folder root path.
#[derive(Default)]
pub struct WorkspaceRegistry {
    pub folders: DashMap<PathBuf, WorkspaceModel>,
}

impl WorkspaceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_folder(&self, root: PathBuf) {
        let model = WorkspaceModel::new(root.clone());
        model.rescan();
        self.folders.insert(root, model);
    }

    pub fn remove_folder(&self, root: &Path) {
        self.folders.remove(root);
    }

    /// Look up the folder containing `path`. Returns the longest-match
    /// folder so nested workspaces resolve to the most-specific root.
    pub fn folder_for(&self, path: &Path) -> Option<WorkspaceModel> {
        let mut best: Option<(PathBuf, WorkspaceModel)> = None;
        for kv in self.folders.iter() {
            if path.starts_with(kv.key()) {
                match &best {
                    None => best = Some((kv.key().clone(), kv.value().clone())),
                    Some((root, _))
                        if kv.key().components().count() > root.components().count() =>
                    {
                        best = Some((kv.key().clone(), kv.value().clone()));
                    }
                    _ => {}
                }
            }
        }
        best.map(|(_, m)| m)
    }

    /// Update an in-memory open-buffer entry across whichever folder
    /// owns it. No-op if no folder contains the path.
    pub fn update_open(&self, uri: &Url, analysis: Arc<DocAnalysis>) {
        let Some(path) = uri_to_path(uri) else {
            return;
        };
        let Some(model) = self.folder_for(&path) else {
            return;
        };
        model.update_open(path, uri.clone(), analysis);
    }

    /// Re-read a file from disk into its owning folder's index. Used
    /// by `didChangeWatchedFiles`.
    pub fn refresh_from_disk(&self, uri: &Url) {
        let Some(path) = uri_to_path(uri) else {
            return;
        };
        if let Some(model) = self.folder_for(&path) {
            model.analyze_path(&path);
        }
    }

    /// Drop a file (e.g. on delete) from its owning folder's index.
    pub fn drop_uri(&self, uri: &Url) {
        let Some(path) = uri_to_path(uri) else {
            return;
        };
        if let Some(model) = self.folder_for(&path) {
            model.drop_path(&path);
        }
    }
}

/// Convert a filesystem path into an `Url` of scheme `file://`.
pub fn path_to_uri(p: &Path) -> Option<Url> {
    Url::from_file_path(p).ok()
}

/// Convert an LSP `Url` back to a filesystem path. Returns None for
/// non-`file` URIs.
pub fn uri_to_path(u: &Url) -> Option<PathBuf> {
    u.to_file_path().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmpdir(prefix: &str) -> PathBuf {
        let pid = std::process::id();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let p = std::env::temp_dir().join(format!("mty-lsp-{prefix}-{pid}-{nonce}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn rescan_indexes_mty_files() {
        let dir = tmpdir("rescan");
        let f = dir.join("a.mty");
        let mut w = std::fs::File::create(&f).unwrap();
        writeln!(w, "fn hello() -> i32 {{ 42 }}").unwrap();

        let model = WorkspaceModel::new(dir.clone());
        model.rescan();
        assert!(model.files.contains_key(&f));
    }

    #[test]
    fn cross_file_refs_returns_matching_occurrences() {
        let dir = tmpdir("crossref");
        let a = dir.join("a.mty");
        let b = dir.join("b.mty");
        std::fs::write(&a, "pub fn shared() -> i32 { 1 }\n").unwrap();
        std::fs::write(&b, "fn caller() -> i32 { shared() }\n").unwrap();

        let model = WorkspaceModel::new(dir);
        model.rescan();
        let hits = model.find_refs_across_files("shared");
        assert!(
            hits.iter().any(|(f, _)| f.path == a),
            "missing decl file: {:?}",
            hits.iter().map(|(f, _)| f.path.clone()).collect::<Vec<_>>()
        );
        assert!(hits.iter().any(|(f, _)| f.path == b), "missing caller file");
    }

    #[test]
    fn skips_target_directory() {
        let dir = tmpdir("skiptarget");
        std::fs::create_dir_all(dir.join("target")).unwrap();
        let inside_target = dir.join("target").join("nope.mty");
        std::fs::write(&inside_target, "fn ignored() {}\n").unwrap();
        let outside = dir.join("ok.mty");
        std::fs::write(&outside, "fn keep() {}\n").unwrap();

        let model = WorkspaceModel::new(dir);
        model.rescan();
        assert!(model.files.contains_key(&outside));
        assert!(!model.files.contains_key(&inside_target));
    }
}
