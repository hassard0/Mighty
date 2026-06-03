//! v0.46 T1 — runtime-ABI introspection surface.
//! v0.47 T3 — `@since` / `@deprecated` markers + numeric version
//! constants.
//!
//! The Mighty compiler emits calls into a fixed family of
//! `mty_runtime_*` C-ABI symbols (see `codegen_abi.rs`). Until v0.46
//! the ground-truth symbol list lived implicitly inside the
//! `#[no_mangle]` attributes and a hand-maintained `symbol_table()`
//! vec; downstream consumers (the IDE shim, agent-emitted native
//! programs, anyone vendoring `mty_rt_abi.lib`) had to rediscover new
//! symbols at link time on every release.
//!
//! v0.46 T1 promotes the list to a generated artifact:
//!
//! - `build.rs` parses `src/codegen_abi.rs` at build time and emits
//!   `crates/mty-runtime/include/mty_runtime_abi.h` (checked-in) plus
//!   `$OUT_DIR/runtime_abi_symbols.rs` (build-private side table).
//! - This module re-exports the side table as
//!   [`RUNTIME_ABI_SIGNATURES`] + [`RUNTIME_ABI_VERSION`] and pins the
//!   generated header bytes via [`RUNTIME_ABI_HEADER`].
//! - `mty abi list / version / header` (see
//!   `crates/mty-cli/src/cmd/abi.rs`) drives off these constants so
//!   every consumer reads the same source of truth.
//!
//! v0.47 T3 extends each [`AbiSignature`] entry with optional
//! `since: Option<&'static str>` and `deprecated: Option<...>`
//! fields populated from `// @since X.Y.Z` / `// @deprecated X.Y.Z`
//! doc comments above each `#[no_mangle]` attribute in
//! `codegen_abi.rs`. The same module also exposes
//! [`RUNTIME_ABI_VERSION_MAJOR`] / `_MINOR` / `_PATCH` for tooling
//! that needs to compare versions numerically.
//!
//! There is also a drift gate: the in-tree header
//! (`include/mty_runtime_abi.h`) ships in the repo so agents can read
//! it without a build, and the `header_matches_in_tree_copy` test in
//! `tests/runtime_abi_header.rs` fails if a swarm-agent adds a new
//! `#[no_mangle]` fn without re-running the build to refresh the
//! header on disk. v0.47 T3 adds a second drift gate:
//! `every_no_mangle_fn_has_since_tag` fails if a new fn is added
//! without a `@since` doc comment.

/// Deprecation marker on an ABI symbol. The `since` field is the
/// release the deprecation landed (NOT the release the symbol
/// originally shipped — that's [`AbiSignature::since`]). The `note`
/// is the optional human-readable hint that follows an em-dash in
/// the source comment (e.g. `// @deprecated 0.47.0 — use
/// mty_runtime_fs_dir_open` produces `Some("use
/// mty_runtime_fs_dir_open")`).
#[derive(Debug, Clone, Copy)]
pub struct AbiDeprecation {
    /// Release the deprecation was declared in.
    pub since: &'static str,
    /// Optional replacement / migration note.
    pub note: Option<&'static str>,
}

/// One entry in the runtime ABI symbol table. Names and types come
/// straight from the Rust signatures in `codegen_abi.rs`; consumers
/// translate to their language's spelling.
#[derive(Debug, Clone, Copy)]
pub struct AbiSignature {
    /// Exported C-ABI symbol name, e.g. `"mty_runtime_log_i32"`.
    pub name: &'static str,
    /// Ordered `(param_name, rust_type)` pairs. The Rust types use
    /// the source spelling (`"i32"`, `"i64"`, `"f64"`, etc.); see
    /// `build.rs::rust_to_c` for the C mapping used in the header.
    pub params: &'static [(&'static str, &'static str)],
    /// Return type spelled in Rust. The sentinel `"()"` means C `void`.
    pub ret: &'static str,
    /// Release tag the symbol was introduced in, sourced from the
    /// `// @since X.Y.Z` doc comment above the attribute. `None`
    /// means the symbol pre-dates the tagging convention (which
    /// shouldn't happen on `main` — the v0.47 T3 drift gate fails
    /// CI if a fn ships without one).
    pub since: Option<&'static str>,
    /// Deprecation marker, if any. `Some(...)` means the symbol is
    /// scheduled for removal — consumers should migrate before the
    /// next major bump. See [`AbiDeprecation`].
    pub deprecated: Option<AbiDeprecation>,
}

// The `include!` brings in:
//   pub const RUNTIME_ABI_VERSION: &str = "...";
//   pub const RUNTIME_ABI_VERSION_MAJOR: u32 = ...;
//   pub const RUNTIME_ABI_VERSION_MINOR: u32 = ...;
//   pub const RUNTIME_ABI_VERSION_PATCH: u32 = ...;
//   pub const RUNTIME_ABI_VERSION_NUMBER: u32 = ...; // MAJOR*10000+MINOR*100+PATCH
//   pub const RUNTIME_ABI_STABILITY: &str = "experimental";
//   pub static RUNTIME_ABI_SIGNATURES: &[AbiSignature] = &[...];
include!(concat!(env!("OUT_DIR"), "/runtime_abi_symbols.rs"));

/// Pinned bytes of the generated `mty_runtime_abi.h` header. Pulled
/// from the same `$OUT_DIR` artifact the C header writer produced, so
/// `mty abi header` cannot drift from what consumers link against.
pub const RUNTIME_ABI_HEADER: &str = include_str!(concat!(env!("OUT_DIR"), "/mty_runtime_abi.h"));

/// Render the signature table as JSON. Used by `mty abi list
/// --format json` and downstream verifier tooling.
///
/// v0.47 T3 adds `since` and `deprecated` fields on each symbol so
/// CI lint scripts can flag agents calling into deprecated symbols.
#[must_use]
pub fn signatures_json() -> String {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str(&format!("  \"version\": \"{}\",\n", RUNTIME_ABI_VERSION));
    s.push_str(&format!(
        "  \"version_major\": {},\n",
        RUNTIME_ABI_VERSION_MAJOR
    ));
    s.push_str(&format!(
        "  \"version_minor\": {},\n",
        RUNTIME_ABI_VERSION_MINOR
    ));
    s.push_str(&format!(
        "  \"version_patch\": {},\n",
        RUNTIME_ABI_VERSION_PATCH
    ));
    s.push_str(&format!(
        "  \"version_number\": {},\n",
        RUNTIME_ABI_VERSION_NUMBER
    ));
    s.push_str(&format!(
        "  \"stability\": \"{}\",\n",
        RUNTIME_ABI_STABILITY
    ));
    s.push_str(&format!("  \"count\": {},\n", RUNTIME_ABI_SIGNATURES.len()));
    s.push_str("  \"symbols\": [\n");
    for (i, sig) in RUNTIME_ABI_SIGNATURES.iter().enumerate() {
        s.push_str("    {");
        s.push_str(&format!(" \"name\": \"{}\",", sig.name));
        s.push_str(" \"params\": [");
        for (j, (pn, pt)) in sig.params.iter().enumerate() {
            if j > 0 {
                s.push_str(", ");
            }
            s.push_str(&format!("{{\"name\": \"{pn}\", \"type\": \"{pt}\"}}"));
        }
        s.push_str("],");
        s.push_str(&format!(" \"ret\": \"{}\",", sig.ret));
        match sig.since {
            Some(v) => s.push_str(&format!(" \"since\": \"{v}\",")),
            None => s.push_str(" \"since\": null,"),
        }
        match sig.deprecated {
            Some(d) => {
                s.push_str(&format!(
                    " \"deprecated\": {{ \"since\": \"{}\", \"note\": ",
                    d.since
                ));
                match d.note {
                    Some(n) => s.push_str(&format!("\"{}\"", json_escape(n))),
                    None => s.push_str("null"),
                }
                s.push_str(" }");
            }
            None => s.push_str(" \"deprecated\": null"),
        }
        s.push_str(" }");
        if i + 1 < RUNTIME_ABI_SIGNATURES.len() {
            s.push(',');
        }
        s.push('\n');
    }
    s.push_str("  ]\n");
    s.push_str("}\n");
    s
}

/// Render the signature table as one-symbol-per-line plain text. Used
/// by `mty abi list` (default format) and the symbol-table drift gate.
///
/// v0.47 T3 appends `# @since X.Y.Z` and (when present) `[deprecated
/// X.Y.Z …]` after each signature so a quick `mty abi list | grep`
/// surfaces deprecated calls.
#[must_use]
pub fn signatures_text() -> String {
    let mut s = String::new();
    for sig in RUNTIME_ABI_SIGNATURES {
        let params = sig
            .params
            .iter()
            .map(|(n, t)| format!("{n}: {t}"))
            .collect::<Vec<_>>()
            .join(", ");
        let ret = if sig.ret == "()" {
            String::new()
        } else {
            format!(" -> {}", sig.ret)
        };
        let mut tail = String::new();
        if let Some(v) = sig.since {
            tail.push_str(&format!("  # @since {v}"));
        }
        if let Some(d) = sig.deprecated {
            tail.push_str(&format!(" [deprecated {}", d.since));
            if let Some(n) = d.note {
                tail.push_str(&format!(" — {n}"));
            }
            tail.push(']');
        }
        s.push_str(&format!("{}({}){}{}\n", sig.name, params, ret, tail));
    }
    s
}

/// Minimal JSON-string escaper for the hand-rolled signature JSON.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_table_is_non_empty() {
        assert!(
            !RUNTIME_ABI_SIGNATURES.is_empty(),
            "build.rs should have produced at least one signature"
        );
    }

    #[test]
    fn signature_names_start_with_mty_runtime() {
        for sig in RUNTIME_ABI_SIGNATURES {
            assert!(
                sig.name.starts_with("mty_runtime_"),
                "unexpected symbol name `{}`",
                sig.name
            );
        }
    }

    #[test]
    fn header_pins_version_macro() {
        let expected = format!(
            "#define MTY_RUNTIME_ABI_VERSION \"{}\"",
            RUNTIME_ABI_VERSION
        );
        assert!(
            RUNTIME_ABI_HEADER.contains(&expected),
            "header should pin the version macro `{expected}`"
        );
    }

    #[test]
    fn header_pins_numeric_version_macros() {
        // v0.47 T3 — three numeric macros so consumers can write
        // `#if MTY_RUNTIME_ABI_VERSION_MINOR >= N`.
        let major = format!(
            "#define MTY_RUNTIME_ABI_VERSION_MAJOR {}",
            RUNTIME_ABI_VERSION_MAJOR
        );
        let minor = format!(
            "#define MTY_RUNTIME_ABI_VERSION_MINOR {}",
            RUNTIME_ABI_VERSION_MINOR
        );
        let patch = format!(
            "#define MTY_RUNTIME_ABI_VERSION_PATCH {}",
            RUNTIME_ABI_VERSION_PATCH
        );
        for needle in [&major, &minor, &patch] {
            assert!(
                RUNTIME_ABI_HEADER.contains(needle),
                "header missing numeric version macro `{needle}`"
            );
        }
    }

    #[test]
    fn header_declares_every_signature() {
        for sig in RUNTIME_ABI_SIGNATURES {
            assert!(
                RUNTIME_ABI_HEADER.contains(sig.name),
                "header missing declaration for `{}`",
                sig.name
            );
        }
    }

    #[test]
    fn header_includes_since_marker_for_each_fn() {
        // v0.47 T3 — every fn with a `since` tag must have a
        // `/* @since X.Y.Z */` comment somewhere in the header. We
        // can't check the comment is on the line directly above the
        // fn declaration here (the renderer guarantees that), but we
        // can check the marker string is present.
        for sig in RUNTIME_ABI_SIGNATURES {
            if let Some(since) = sig.since {
                let needle = format!("@since {since}");
                assert!(
                    RUNTIME_ABI_HEADER.contains(&needle),
                    "header missing `{needle}` marker for `{}`",
                    sig.name
                );
            }
        }
    }

    #[test]
    fn json_renders_valid_brace_structure() {
        let j = signatures_json();
        // Sanity-only — the renderer is hand-rolled, so check the
        // outer shape stays well-formed.
        assert!(j.starts_with("{\n"));
        assert!(j.trim_end().ends_with('}'));
        assert!(j.contains("\"symbols\""));
        assert!(j.contains("\"version\""));
        // v0.47 T3 — numeric version fields + per-symbol since/deprecated.
        assert!(j.contains("\"version_major\""));
        assert!(j.contains("\"version_minor\""));
        assert!(j.contains("\"version_patch\""));
        assert!(j.contains("\"since\""));
        assert!(j.contains("\"deprecated\""));
    }

    #[test]
    fn fs_read_dir_is_marked_deprecated() {
        // v0.46 T4 introduced the iterator-handle ABI; v0.47 T3
        // formalizes the read_dir deprecation. Lock it in.
        let read_dir = RUNTIME_ABI_SIGNATURES
            .iter()
            .find(|s| s.name == "mty_runtime_fs_read_dir")
            .expect("mty_runtime_fs_read_dir should still be in the surface");
        let dep = read_dir
            .deprecated
            .expect("mty_runtime_fs_read_dir should carry an @deprecated marker");
        assert_eq!(dep.since, "0.47.0");
        let note = dep
            .note
            .expect("the deprecation should point at the replacement");
        assert!(
            note.contains("mty_runtime_fs_dir_open"),
            "deprecation note should mention the replacement, got `{note}`"
        );
    }

    #[test]
    fn drift_gate_matches_symbol_table_entries() {
        // Every symbol in the legacy hand-maintained `symbol_table()`
        // must appear in the generated `RUNTIME_ABI_SIGNATURES`. If
        // an agent adds a `#[no_mangle]` fn to `codegen_abi.rs` and
        // updates `symbol_table()` but not the generated header,
        // this test still passes — the generator runs from the same
        // source file. If an agent adds the `#[no_mangle]` fn but
        // forgets `symbol_table()`, this test fails loudly.
        use crate::codegen_abi::symbol_table;
        let st: std::collections::HashSet<String> =
            symbol_table().into_iter().map(|(n, _)| n).collect();
        let gen_names: std::collections::HashSet<String> = RUNTIME_ABI_SIGNATURES
            .iter()
            .map(|s| s.name.to_string())
            .collect();
        let only_in_gen: Vec<_> = gen_names.difference(&st).collect();
        let only_in_st: Vec<_> = st.difference(&gen_names).collect();
        assert!(
            only_in_gen.is_empty() && only_in_st.is_empty(),
            "runtime ABI drift — in-source symbol_table() vs generated header:\n  \
             only in generated:   {only_in_gen:?}\n  \
             only in symbol_table: {only_in_st:?}"
        );
    }
}
