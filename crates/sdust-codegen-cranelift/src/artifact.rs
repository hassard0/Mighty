//! Compiled-artifact descriptors. The driver hands these back to the
//! CLI so it can print useful "wrote /path/to/binary" messages.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct NativeArtifact {
    /// Path to the final executable (post-link).
    pub binary_path: PathBuf,
    /// Path to the intermediate object file (kept for debugging).
    pub object_path: Option<PathBuf>,
    /// Compilation mode used.
    pub mode: BuildMode,
    /// Triple of the host target.
    pub target_triple: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMode {
    Debug,
    Release,
}

impl BuildMode {
    pub fn as_str(self) -> &'static str {
        match self {
            BuildMode::Debug => "debug",
            BuildMode::Release => "release",
        }
    }
}
