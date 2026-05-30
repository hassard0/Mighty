//! Linker-flavor detection and arg rewriting (v0.41 T4).
//!
//! The build driver speaks GNU-ld syntax internally (`-lfoo`,
//! `-L/path`, `--gc-sections`). When the host linker is actually MSVC's
//! `link.exe` (or lld's `lld-link.exe`), the arg vector has to be
//! translated before the [`std::process::Command`] is spawned. Doing the
//! translation in one place — [`rewrite_for_flavor`] — keeps the
//! manifest schema portable and centralises the per-linker quirks.
//!
//! Detection is best-effort: we look at the resolved linker path's
//! basename and match a small set of MSVC-ish names. If `MTY_LINKER` /
//! `STARDUST_LINKER` point at a custom wrapper (`my-msvc-shim.exe`),
//! callers can set `MTY_LINKER_FLAVOR=msvc` to force the rewrite. This
//! also gives users a manual override when our heuristic guesses wrong.
//!
//! See `docs/internals/extern-c-matrix.md` for the per-shape examples
//! and the user-facing flag matrix.

/// Which linker family the driver is talking to. Determines whether
/// the arg vector needs to be rewritten from GNU shape to MSVC shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkerFlavor {
    /// `clang`, `gcc`, `cc`, `ld.lld`, `lld` — accept GNU-ld syntax.
    #[default]
    Gnu,
    /// MSVC `link.exe` or LLVM's `lld-link.exe` — speak `/SWITCH:VALUE`.
    Msvc,
}

impl LinkerFlavor {
    /// Detect the flavor for `path`. Honours the
    /// `MTY_LINKER_FLAVOR=msvc|gnu` override first, then falls back to
    /// a basename heuristic. Unknown linkers default to [`Self::Gnu`]
    /// (the wider-compat choice).
    ///
    /// `path` is whatever
    /// [`mty_codegen_cranelift::object::find_linker`] returned — may be
    /// a bare name (`"clang"`), an absolute path, or a custom wrapper.
    pub fn detect(path: &str) -> Self {
        if let Ok(v) = std::env::var("MTY_LINKER_FLAVOR") {
            match v.trim().to_ascii_lowercase().as_str() {
                "msvc" | "link" | "link.exe" => return LinkerFlavor::Msvc,
                "gnu" | "ld" | "lld" => return LinkerFlavor::Gnu,
                _ => {} // ignore garbage values, fall through to heuristic
            }
        }
        Self::detect_from_path(path)
    }

    /// Pure heuristic on the basename. Public for unit tests and for
    /// callers that have already consulted the env override.
    pub fn detect_from_path(path: &str) -> Self {
        // Lowercase basename, no extension.
        let basename = path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(path)
            .to_ascii_lowercase();
        let stem = basename
            .strip_suffix(".exe")
            .unwrap_or(&basename)
            .to_string();
        match stem.as_str() {
            // Bare MSVC linker names.
            "link" | "lld-link" => LinkerFlavor::Msvc,
            // Everything else (clang, clang-cl, gcc, cc, ld.lld, lld, …)
            // speaks GNU-ld syntax for our needs. Note: `clang-cl` does
            // accept MSVC syntax but also accepts GNU-style on Windows
            // when invoked as a *frontend* (it forwards to lld-link).
            // The safer default is to feed it GNU shape and let clang
            // translate.
            _ => LinkerFlavor::Gnu,
        }
    }
}

/// Rewrite a flat GNU-shape arg vector for the target linker flavor.
///
/// Translation table (GNU → MSVC):
///
/// | GNU shape | MSVC shape |
/// |-----------|------------|
/// | `-lfoo` | `foo.lib` |
/// | `-Lpath` | `/LIBPATH:path` |
/// | `-L path` (split) | `/LIBPATH:path` |
/// | `--gc-sections` / `-Wl,--gc-sections` | `/OPT:REF` |
/// | `-Wl,-rpath,/x` | dropped (MSVC has no rpath) |
/// | `-framework` `Name` (pair) | dropped (no Windows analogue) |
/// | anything else | passed through unchanged |
///
/// GNU flavor is identity (the input vector is returned verbatim).
///
/// The function is total: unknown flags pass through so user escape
/// hatches keep working. Callers needing the rewrite to be loud about
/// the dropped flags can compare lengths.
pub fn rewrite_for_flavor(args: &[String], flavor: LinkerFlavor) -> Vec<String> {
    if matches!(flavor, LinkerFlavor::Gnu) {
        return args.to_vec();
    }
    let mut out: Vec<String> = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        // `-lname` (no space): translate to `name.lib`.
        if let Some(rest) = a.strip_prefix("-l") {
            if !rest.is_empty() {
                out.push(format!("{rest}.lib"));
                i += 1;
                continue;
            }
        }
        // `-Lpath` (no space): translate to `/LIBPATH:path`.
        if let Some(rest) = a.strip_prefix("-L") {
            if !rest.is_empty() {
                out.push(format!("/LIBPATH:{rest}"));
                i += 1;
                continue;
            }
            // `-L path` (split): consume next arg.
            if let Some(next) = args.get(i + 1) {
                out.push(format!("/LIBPATH:{next}"));
                i += 2;
                continue;
            }
        }
        // `--gc-sections` (or `-Wl,--gc-sections`) → `/OPT:REF`.
        if a == "--gc-sections" || a == "-Wl,--gc-sections" {
            out.push("/OPT:REF".to_string());
            i += 1;
            continue;
        }
        // rpath has no MSVC analogue — drop.
        if a.starts_with("-Wl,-rpath") || a.starts_with("-rpath") {
            i += 1;
            continue;
        }
        // `-framework Name` pair has no Windows analogue — drop both.
        if a == "-framework" {
            i += if args.get(i + 1).is_some() { 2 } else { 1 };
            continue;
        }
        // Default: pass through. This keeps the escape hatch open for
        // raw MSVC flags users already encoded in `link-args`.
        out.push(a.clone());
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // We can't reliably mutate process env in parallel cargo tests, so
    // the flavor-from-env tests live in driver/tests/manifest_build.rs
    // alongside other env-touching cases. These unit tests cover the
    // pure-string heuristic.

    #[test]
    fn detect_msvc_from_path() {
        assert_eq!(
            LinkerFlavor::detect_from_path("C:\\Program Files\\MSVC\\bin\\link.exe"),
            LinkerFlavor::Msvc
        );
        assert_eq!(
            LinkerFlavor::detect_from_path("link.exe"),
            LinkerFlavor::Msvc
        );
        assert_eq!(LinkerFlavor::detect_from_path("link"), LinkerFlavor::Msvc);
        assert_eq!(
            LinkerFlavor::detect_from_path("lld-link.exe"),
            LinkerFlavor::Msvc
        );
        assert_eq!(
            LinkerFlavor::detect_from_path("/usr/local/bin/lld-link"),
            LinkerFlavor::Msvc
        );
    }

    #[test]
    fn detect_gnu_from_path() {
        for p in [
            "clang",
            "clang.exe",
            "gcc",
            "cc",
            "/usr/bin/clang",
            "C:\\msys64\\mingw64\\bin\\gcc.exe",
            "ld.lld",
            "lld",
        ] {
            assert_eq!(
                LinkerFlavor::detect_from_path(p),
                LinkerFlavor::Gnu,
                "expected Gnu for {p}"
            );
        }
    }

    #[test]
    fn rewrite_gnu_is_identity() {
        let args = vec!["-lfoo".to_string(), "-L/x".to_string()];
        assert_eq!(rewrite_for_flavor(&args, LinkerFlavor::Gnu), args);
    }

    #[test]
    fn rewrite_msvc_translates_l_and_dash_l() {
        let args = vec![
            "-L/opt/lib".to_string(),
            "-lfoo".to_string(),
            "-lbar".to_string(),
        ];
        let got = rewrite_for_flavor(&args, LinkerFlavor::Msvc);
        assert_eq!(
            got,
            vec![
                "/LIBPATH:/opt/lib".to_string(),
                "foo.lib".to_string(),
                "bar.lib".to_string(),
            ]
        );
    }

    #[test]
    fn rewrite_msvc_handles_split_dash_l() {
        let args = vec!["-L".to_string(), "/opt/lib".to_string(), "-lz".to_string()];
        let got = rewrite_for_flavor(&args, LinkerFlavor::Msvc);
        assert_eq!(
            got,
            vec!["/LIBPATH:/opt/lib".to_string(), "z.lib".to_string()]
        );
    }

    #[test]
    fn rewrite_msvc_gc_sections() {
        let args = vec![
            "--gc-sections".to_string(),
            "-Wl,--gc-sections".to_string(),
            "-lz".to_string(),
        ];
        let got = rewrite_for_flavor(&args, LinkerFlavor::Msvc);
        assert_eq!(
            got,
            vec![
                "/OPT:REF".to_string(),
                "/OPT:REF".to_string(),
                "z.lib".to_string()
            ]
        );
    }

    #[test]
    fn rewrite_msvc_drops_framework_pair() {
        let args = vec![
            "-framework".to_string(),
            "Cocoa".to_string(),
            "-lz".to_string(),
        ];
        let got = rewrite_for_flavor(&args, LinkerFlavor::Msvc);
        // Framework pair dropped; `-lz` translated.
        assert_eq!(got, vec!["z.lib".to_string()]);
    }

    #[test]
    fn rewrite_msvc_drops_rpath() {
        let args = vec![
            "-Wl,-rpath,/x".to_string(),
            "-rpath".to_string(),
            "-lz".to_string(),
        ];
        let got = rewrite_for_flavor(&args, LinkerFlavor::Msvc);
        assert_eq!(got, vec!["z.lib".to_string()]);
    }

    #[test]
    fn rewrite_msvc_passes_unknown_through() {
        // A raw MSVC switch encoded directly in link-args must survive
        // the rewrite untouched — it's the escape hatch.
        let args = vec!["/SUBSYSTEM:CONSOLE".to_string(), "foo.lib".to_string()];
        let got = rewrite_for_flavor(&args, LinkerFlavor::Msvc);
        assert_eq!(got, args);
    }

    #[test]
    fn rewrite_msvc_preserves_order() {
        let args = vec![
            "-L/a".to_string(),
            "-lfirst".to_string(),
            "-L/b".to_string(),
            "-lsecond".to_string(),
        ];
        let got = rewrite_for_flavor(&args, LinkerFlavor::Msvc);
        assert_eq!(
            got,
            vec![
                "/LIBPATH:/a".to_string(),
                "first.lib".to_string(),
                "/LIBPATH:/b".to_string(),
                "second.lib".to_string(),
            ]
        );
    }

    #[test]
    fn detect_falls_back_to_gnu_on_garbage() {
        assert_eq!(LinkerFlavor::detect_from_path(""), LinkerFlavor::Gnu);
        assert_eq!(
            LinkerFlavor::detect_from_path("totally-custom-wrapper"),
            LinkerFlavor::Gnu
        );
    }
}
