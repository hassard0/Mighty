//! `mty new <name>` — scaffold a new Mighty package.
//!
//! v0.23 Track C adds an optional `--template <name>` flag and a
//! template registry. The default ("blank") template is the original
//! v0.1 two-file scaffold (`mighty.toml` + `src/main.mty` with a
//! `log("hello, Mighty")` body). Additional templates are embedded
//! at compile time from `crates/mty-cli/templates/<name>/` via
//! `include_str!`. See `dev/history/notes/MTY_SERVE_V0_23_NOTES.md`.
//!
//! Add a new template by:
//!   1. dropping its files under `crates/mty-cli/templates/<name>/`,
//!   2. adding a `Template` entry to `TEMPLATES` below,
//!   3. growing the `cmd_new_template.rs` integration test.

use std::fs;
use std::path::{Path, PathBuf};

/// One file inside a template. `path` is the destination path
/// *relative* to the package root; `content` is the source text
/// embedded at compile time. `{{NAME}}` placeholders are substituted
/// with the user-supplied package name at scaffold time.
struct TemplateFile {
    /// Path relative to the new package directory.
    path: &'static str,
    /// Source text with `{{NAME}}` substitution applied at write time.
    content: &'static str,
}

struct Template {
    /// Flag value the user passes to `--template`.
    name: &'static str,
    /// One-line description (for `mty new --help` listings if we
    /// ever surface this; currently used only by tests).
    #[allow(dead_code)]
    description: &'static str,
    files: &'static [TemplateFile],
}

// ----------------------------------------------------------------
// Template registry. Order is stable — `default_template()` returns
// the first entry.
// ----------------------------------------------------------------

const BLANK_MANIFEST: &str = r#"[package]
name = "{{NAME}}"
version = "0.1.0"
edition = "2026"
profile = "host"

[deps]
"#;

const BLANK_MAIN: &str = "fn main() {\n  log(\"hello, Mighty\")\n}\n";

const BLANK_FILES: &[TemplateFile] = &[
    TemplateFile {
        path: "mighty.toml",
        content: BLANK_MANIFEST,
    },
    TemplateFile {
        path: "src/main.mty",
        content: BLANK_MAIN,
    },
];

const WEB_GAME_FILES: &[TemplateFile] = &[
    TemplateFile {
        path: "mighty.toml",
        content: include_str!("../../templates/web-game/mighty.toml"),
    },
    TemplateFile {
        path: "src/main.mty",
        content: include_str!("../../templates/web-game/src/main.mty"),
    },
    TemplateFile {
        path: "web/index.html",
        content: include_str!("../../templates/web-game/web/index.html"),
    },
    TemplateFile {
        path: "web/dom-shim.js",
        content: include_str!("../../templates/web-game/web/dom-shim.js"),
    },
    TemplateFile {
        path: "README.md",
        content: include_str!("../../templates/web-game/README.md"),
    },
];

const TEMPLATES: &[Template] = &[
    Template {
        name: "blank",
        description: "Minimal package: mighty.toml + src/main.mty.",
        files: BLANK_FILES,
    },
    Template {
        name: "web-game",
        description: "Browser-hosted game scaffold: agent + canvas + dom-shim.",
        files: WEB_GAME_FILES,
    },
];

fn find_template(name: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.name == name)
}

fn default_template() -> &'static Template {
    &TEMPLATES[0]
}

/// Substitute `{{NAME}}` with the user-supplied package name.
fn substitute(content: &str, pkg_name: &str) -> String {
    content.replace("{{NAME}}", pkg_name)
}

/// Derive a valid Mighty package identifier from a user-supplied path.
///
/// `mty new <path>` accepts either a bare name (`asteroids`) or a
/// directory path (`/tmp/asteroids`, `C:/Users/.../asteroids`). The
/// `{{NAME}}` substitution must always be a valid identifier, so we
/// take the path's last component and then sanitize it: keep ASCII
/// alphanumerics + underscores, lowercase, replace anything else with
/// `_`, and ensure the first character isn't a digit.
fn package_name_from_path(raw: &str) -> String {
    // Use the filesystem basename rather than the raw arg, so
    // `mty new C:/tmp/foo` becomes `foo` not `C:/tmp/foo`.
    let basename = Path::new(raw)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| raw.to_string());

    let mut s: String = basename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        s.push_str("pkg");
    }
    // Identifiers can't start with a digit.
    if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    s
}

/// Scaffold a fresh package directory. `template` is the
/// `--template <name>` argument (None ⇒ the default blank template).
pub fn run(name: &str, template: Option<&str>) -> i32 {
    let tpl = match template {
        Some(t) => match find_template(t) {
            Some(found) => found,
            None => {
                eprintln!(
                    "unknown --template `{}` (available: {})",
                    t,
                    TEMPLATES
                        .iter()
                        .map(|x| x.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return 2;
            }
        },
        None => default_template(),
    };

    let dir = Path::new(name);
    if dir.exists() {
        eprintln!("directory `{}` already exists", name);
        return 1;
    }
    if let Err(e) = fs::create_dir_all(dir) {
        eprintln!("failed to create directory: {}", e);
        return 1;
    }

    // The `name` arg can be a path; derive a valid identifier for
    // `package {{NAME}}` substitution from its basename.
    let pkg_id = package_name_from_path(name);

    for f in tpl.files {
        let out: PathBuf = dir.join(f.path);
        if let Some(parent) = out.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                eprintln!("failed to create {}: {}", parent.display(), e);
                return 1;
            }
        }
        let body = substitute(f.content, &pkg_id);
        if let Err(e) = fs::write(&out, body) {
            eprintln!("failed to write {}: {}", out.display(), e);
            return 1;
        }
    }

    println!(
        "created {}/ (template: {}, package: {})",
        name, tpl.name, pkg_id
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_template_is_blank() {
        assert_eq!(default_template().name, "blank");
    }

    #[test]
    fn web_game_template_resolves() {
        let t = find_template("web-game").expect("web-game template registered");
        // Must ship the 4 files the spec calls for (+ README).
        let paths: Vec<&str> = t.files.iter().map(|f| f.path).collect();
        assert!(paths.contains(&"src/main.mty"));
        assert!(paths.contains(&"web/index.html"));
        assert!(paths.contains(&"web/dom-shim.js"));
        assert!(paths.contains(&"mighty.toml"));
    }

    #[test]
    fn unknown_template_is_rejected() {
        assert!(find_template("nope").is_none());
    }

    #[test]
    fn substitute_replaces_all_placeholders() {
        let s = substitute("name={{NAME}}; again={{NAME}}", "foo");
        assert_eq!(s, "name=foo; again=foo");
    }

    #[test]
    fn package_name_strips_path_prefix() {
        assert_eq!(package_name_from_path("asteroids"), "asteroids");
        assert_eq!(package_name_from_path("/tmp/asteroids"), "asteroids");
        assert_eq!(
            package_name_from_path("C:/Users/x/AppData/Local/Temp/asteroids"),
            "asteroids"
        );
    }

    #[test]
    fn package_name_sanitises_punctuation_and_case() {
        assert_eq!(package_name_from_path("My-Web Game"), "my_web_game");
        assert_eq!(package_name_from_path("foo.bar"), "foo_bar");
    }

    #[test]
    fn package_name_prepends_underscore_for_leading_digit() {
        assert_eq!(package_name_from_path("3d-engine"), "_3d_engine");
    }

    #[test]
    fn package_name_falls_back_to_pkg_for_empty_input() {
        assert_eq!(package_name_from_path(""), "pkg");
    }
}
