//! v0.46 T1 — runtime-ABI introspection surface.
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
//! There is also a drift gate: the in-tree header
//! (`include/mty_runtime_abi.h`) ships in the repo so agents can read
//! it without a build, and the `header_matches_in_tree_copy` test in
//! `tests/runtime_abi_header.rs` fails if a swarm-agent adds a new
//! `#[no_mangle]` fn without re-running the build to refresh the
//! header on disk.

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
}

// The `include!` brings in:
//   pub const RUNTIME_ABI_VERSION: &str = "...";
//   pub static RUNTIME_ABI_SIGNATURES: &[AbiSignature] = &[...];
include!(concat!(env!("OUT_DIR"), "/runtime_abi_symbols.rs"));

/// Pinned bytes of the generated `mty_runtime_abi.h` header. Pulled
/// from the same `$OUT_DIR` artifact the C header writer produced, so
/// `mty abi header` cannot drift from what consumers link against.
pub const RUNTIME_ABI_HEADER: &str = include_str!(concat!(env!("OUT_DIR"), "/mty_runtime_abi.h"));

/// Render the signature table as JSON. Used by `mty abi list
/// --format json` and downstream verifier tooling.
#[must_use]
pub fn signatures_json() -> String {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str(&format!("  \"version\": \"{}\",\n", RUNTIME_ABI_VERSION));
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
        s.push_str(&format!(" \"ret\": \"{}\"", sig.ret));
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
        s.push_str(&format!("{}({}){}\n", sig.name, params, ret));
    }
    s
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
    fn json_renders_valid_brace_structure() {
        let j = signatures_json();
        // Sanity-only — the renderer is hand-rolled, so check the
        // outer shape stays well-formed.
        assert!(j.starts_with("{\n"));
        assert!(j.trim_end().ends_with('}'));
        assert!(j.contains("\"symbols\""));
        assert!(j.contains("\"version\""));
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
