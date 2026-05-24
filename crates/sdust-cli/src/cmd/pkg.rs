//! `sdust pkg <subcmd>` — package manager CLI surface.
//!
//! Thin shim over `sdust_pkg::commands::*`. All errors are reported to
//! stderr and the process exits non-zero.

use clap::Subcommand;
use sdust_pkg::commands;
use sdust_pkg::DetailedDep;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum PkgCmd {
    /// Add a dependency to `star.toml` and update the lockfile.
    Add {
        /// Package name. Optionally `name@version` for the registry.
        spec: String,
        /// Override version (alternative to `name@version`).
        #[arg(long)]
        version: Option<String>,
        /// Use a local path source.
        #[arg(long)]
        path: Option<String>,
        /// Use a git source.
        #[arg(long)]
        git: Option<String>,
        /// Pin a specific git rev / tag / branch.
        #[arg(long)]
        rev: Option<String>,
    },
    /// Remove a dependency from `star.toml` and update the lockfile.
    Remove {
        /// Package name.
        name: String,
    },
    /// Re-resolve dependencies (optionally restricted to one name).
    Update {
        /// Restrict to a single package.
        name: Option<String>,
    },
    /// Materialise all locked dependencies into `.stardust/pkgs/`.
    Fetch,
    /// Print the resolved dependency tree.
    List,
    /// Bundle the current package for publishing.
    ///
    /// v0.2 produces a local bundle only — the Stardust registry is
    /// not yet live.
    Publish,
}

pub fn run(cmd: PkgCmd, root: Option<PathBuf>) -> i32 {
    let root = root.unwrap_or_else(|| std::env::current_dir().unwrap());
    let result: Result<String, commands::PkgError> = match cmd {
        PkgCmd::Add {
            spec,
            version,
            path,
            git,
            rev,
        } => {
            // Parse `name@version` if no explicit `--version`.
            let (name, parsed_version) = match spec.split_once('@') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (spec, None),
            };
            let ver = version.or(parsed_version);
            if path.is_some() || git.is_some() {
                let detailed = DetailedDep {
                    version: ver,
                    path,
                    git,
                    rev,
                    hash: None,
                };
                commands::add_detailed(&root, &name, detailed)
            } else {
                commands::add(&root, &name, ver.as_deref())
            }
        }
        PkgCmd::Remove { name } => commands::remove(&root, &name),
        PkgCmd::Update { name } => commands::update(&root, name.as_deref()),
        PkgCmd::Fetch => {
            commands::fetch_all(&root).map(|v| format!("fetched {} package(s)", v.len()))
        }
        PkgCmd::List => commands::list(&root),
        PkgCmd::Publish => commands::publish(&root),
    };

    match result {
        Ok(msg) => {
            print!("{msg}");
            if !msg.ends_with('\n') {
                println!();
            }
            0
        }
        Err(e) => {
            eprintln!("sdust pkg: {e}");
            1
        }
    }
}
