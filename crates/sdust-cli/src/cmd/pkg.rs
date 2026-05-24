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
        /// Refresh cached registry indexes from GitHub before
        /// re-resolving.
        #[arg(long)]
        refresh: bool,
    },
    /// Materialise all locked dependencies into `.stardust/pkgs/`.
    Fetch,
    /// Print the resolved dependency tree.
    List,
    /// Search cached registry indexes by substring.
    Search {
        /// Substring to match against package name and version.
        query: String,
    },
    /// Show metadata for a published package.
    Info {
        /// `<name>` or `<name>@<version>`.
        spec: String,
    },
    /// Store a GitHub token for a registry.
    ///
    /// Pass the token via env-var `SDUST_PKG_LOGIN_TOKEN`. The token
    /// is persisted to `~/.config/sdust/auth.toml` (`0600` on Unix).
    Login {
        /// Registry slug `<owner>/<repo>`; defaults to the configured
        /// `[registry].default` (or the official Stardust registry).
        registry: Option<String>,
    },
    /// Bundle the current package and (when authed) upload it to the
    /// default registry as a GitHub Release.
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
        PkgCmd::Update { name, refresh } => commands::update(&root, name.as_deref(), refresh),
        PkgCmd::Fetch => {
            commands::fetch_all(&root).map(|v| format!("fetched {} package(s)", v.len()))
        }
        PkgCmd::List => commands::list(&root),
        PkgCmd::Search { query } => commands::search(&root, &query),
        PkgCmd::Info { spec } => commands::info(&root, &spec),
        PkgCmd::Login { registry } => commands::login(registry.as_deref(), &root),
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
