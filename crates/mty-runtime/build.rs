// v0.46 T1 — Runtime ABI artifact pipeline.
// v0.47 T3 — stability markers + numeric version macros.
//
// The Mighty compiler emits calls into a fixed family of
// `mty_runtime_*` C-ABI symbols (declared in
// `src/codegen_abi.rs`). Every release adds more — by v0.45 the
// surface had grown to ~50 fns covering logging, typed-print, format,
// string concat, and the v0.45 T1 native `std.fs.*` family. Consumers
// who link the static runtime (the IDE shim, agent-emitted native
// programs, downstream `extern c` callers) currently mirror the
// symbol list by hand and silently miss new entries until link-time
// undefined-symbol errors.
//
// v0.46 T1 fixes this once and for all by:
//
//   1. Parsing `src/codegen_abi.rs` at build time, extracting every
//      `#[no_mangle] pub extern "C" fn mty_runtime_*` signature.
//   2. Emitting an official C header
//      `include/mty_runtime_abi.h` checked into the repo so agents
//      browsing the source see it directly.
//   3. Generating a side table at `$OUT_DIR/runtime_abi_symbols.rs`
//      that the runtime re-exports through `crate::abi_export`, so
//      `mty abi list` and downstream tooling can verify against the
//      ground truth without re-parsing source.
//   4. Producing `$OUT_DIR/runtime_abi.h` as a build-cache copy used by
//      `mty abi header` and packaged from `target-t1/<profile>/build/
//      mty-runtime-*/out/runtime_abi.h` into the release tarball.
//
// v0.47 T3 extends the pipeline with two consumer-side affordances:
//
//   5. The parser now picks up `// @since X.Y.Z` and
//      `// @deprecated X.Y.Z[ — note]` doc comments that precede each
//      `#[no_mangle]` attribute. These are emitted as
//      `/* @since X.Y.Z */` comments above the C declaration so
//      downstream consumers reading the header can see the API age
//      and any planned removal at a glance. They also flow through
//      into `RUNTIME_ABI_SIGNATURES.since` / `.deprecated`, so
//      `mty abi list` and the JSON output expose them too.
//   6. The header now defines three numeric version macros
//      (`MTY_RUNTIME_ABI_VERSION_MAJOR/MINOR/PATCH`) alongside the
//      existing string macro, so downstream consumers can write
//      `#if MTY_RUNTIME_ABI_VERSION_MINOR >= 46` compat checks. The
//      values come from the same version string the rest of the
//      pipeline pins; the build.rs splits it on '.' and emits each
//      component as an unsuffixed integer literal.
//
// The check-in copy under `crates/mty-runtime/include/` is the
// source-of-truth artifact (so users can `git diff` it across
// releases). The build script re-emits it on every build; if a track
// adds a new `#[no_mangle]` fn without committing the regenerated
// header, the build STILL succeeds and updates the file on disk —
// but our `runtime_abi_header_in_sync` test (see
// `tests/runtime_abi_header.rs`) then fails because the checked-in
// copy on `git` lags the generator output. This makes drift visible
// without breaking incremental dev.
//
// v0.47 T3 also adds a stricter drift gate that fails the build if a
// new `#[no_mangle]` fn lacks a `@since` doc comment — see
// `tests/runtime_abi_header.rs::every_no_mangle_fn_has_since_tag`.

use std::fs;
use std::path::{Path, PathBuf};

/// Header-level stability marker for the whole runtime ABI. Pre-1.0:
/// the symbol surface may grow / deprecate / reshape between minor
/// releases, so we advertise `"experimental"` until a v1.0 freeze.
/// Emitted both as the C `MTY_RUNTIME_ABI_STABILITY` macro and the
/// Rust `RUNTIME_ABI_STABILITY` constant so both consumer sides agree.
const ABI_STABILITY: &str = "experimental";

fn main() {
    let manifest_dir = PathBuf::from(env_or_panic("CARGO_MANIFEST_DIR"));
    let abi_src = manifest_dir.join("src").join("codegen_abi.rs");
    println!("cargo:rerun-if-changed={}", abi_src.display());
    println!("cargo:rerun-if-changed=build.rs");
    // Tagged-release CI sets MTY_ABI_VERSION to pin the artifact
    // version (`v0.46.0` → `0.46.0`); dev builds leave it unset and
    // pick up `MIGHTY_VERSION` from `crates/mty-cli/src/lib.rs`. Both
    // paths must rebuild the generated header when the env flips so
    // cached artifacts don't survive a release-tag bump.
    println!("cargo:rerun-if-env-changed=MTY_ABI_VERSION");

    // Resolve the user-facing toolchain version. The workspace
    // `version.workspace = true` pin is still `0.1.0` (a holdover
    // from pre-v0.2 when the crates and the toolchain shared a
    // number), so we deliberately don't use `CARGO_PKG_VERSION` for
    // the ABI artifact. The canonical version string is
    // `mty_cli::MIGHTY_VERSION` (e.g. `"0.45.0"`, bumped each
    // release). We re-read it from `crates/mty-cli/src/lib.rs` rather
    // than depending on `mty-cli` (circular: mty-cli depends on
    // mty-runtime). An env override (`MTY_ABI_VERSION`) is honored
    // for tooling that wants to pin a specific tag in CI artifacts.
    let version = match std::env::var("MTY_ABI_VERSION") {
        Ok(v) if !v.is_empty() => v,
        _ => resolve_mighty_version(&manifest_dir),
    };
    let src = fs::read_to_string(&abi_src)
        .unwrap_or_else(|e| panic!("read {}: {}", abi_src.display(), e));

    let fns = parse_abi(&src);
    assert!(
        !fns.is_empty(),
        "build.rs: no `mty_runtime_*` extern \"C\" fns found in {}",
        abi_src.display()
    );

    // v0.47 T3 — drift gate: every fn must carry a `@since` doc
    // comment. The integration test surfaces this with a clearer
    // diagnostic, but we also surface it as a build warning here so
    // an agent running `cargo build -p mty-runtime` sees it
    // immediately. We don't fail the build (incremental dev with a
    // half-written fn shouldn't break) — the test is the gate.
    let missing_since: Vec<&str> = fns
        .iter()
        .filter(|f| f.since.is_none())
        .map(|f| f.name.as_str())
        .collect();
    if !missing_since.is_empty() {
        println!(
            "cargo:warning=mty-runtime ABI: {} fn(s) missing `// @since` doc comment: {:?}",
            missing_since.len(),
            missing_since
        );
    }

    // --- Emit the header file (in-tree, checked-in, plus OUT_DIR copy).
    let header = render_header(&fns, &version);

    let in_tree_header = manifest_dir.join("include").join("mty_runtime_abi.h");
    if let Some(parent) = in_tree_header.parent() {
        fs::create_dir_all(parent).expect("mkdir include/");
    }
    // Write only if changed so we don't churn mtimes on every build.
    write_if_changed(&in_tree_header, &header);

    let out_dir = PathBuf::from(env_or_panic("OUT_DIR"));
    fs::write(out_dir.join("mty_runtime_abi.h"), &header).expect("write OUT_DIR header");

    // --- Emit the symbol-table side file consumed by abi_export.rs.
    let table = render_symbol_table(&fns, &version);
    fs::write(out_dir.join("runtime_abi_symbols.rs"), &table).expect("write OUT_DIR symbol table");
}

/// Walk up from `crates/mty-runtime/` to the workspace root and
/// extract `MIGHTY_VERSION` from `crates/mty-cli/src/lib.rs`. Returns
/// `"0.0.0-dev"` if the file is unreadable (e.g. when the crate is
/// vendored without its sibling).
fn resolve_mighty_version(manifest_dir: &Path) -> String {
    let cli_lib = manifest_dir
        .parent()
        .map(|p| p.join("mty-cli").join("src").join("lib.rs"));
    let Some(cli_lib) = cli_lib else {
        return "0.0.0-dev".to_string();
    };
    println!("cargo:rerun-if-changed={}", cli_lib.display());
    let Ok(src) = fs::read_to_string(&cli_lib) else {
        return "0.0.0-dev".to_string();
    };
    for line in src.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("pub const MIGHTY_VERSION: &str = \"") {
            if let Some(end) = rest.find('"') {
                return rest[..end].to_string();
            }
        }
    }
    "0.0.0-dev".to_string()
}

/// Split a version string like `"0.47.0"` into `(major, minor,
/// patch)`. Non-numeric components fall back to `0`. Pre-release
/// suffixes (`"0.47.0-rc1"`) are stripped from the patch component
/// before parsing so the resulting macros are valid C integer
/// literals.
fn split_version(v: &str) -> (u32, u32, u32) {
    let mut parts = v.split('.');
    let major = parts.next().unwrap_or("0");
    let minor = parts.next().unwrap_or("0");
    let patch = parts.next().unwrap_or("0");
    // Strip suffixes like `-rc1` / `+build` off the patch.
    let patch_clean: String = patch.chars().take_while(|c| c.is_ascii_digit()).collect();
    (
        major.parse().unwrap_or(0),
        minor.parse().unwrap_or(0),
        patch_clean.parse().unwrap_or(0),
    )
}

fn env_or_panic(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("missing env var {key}"))
}

fn write_if_changed(path: &Path, contents: &str) {
    let cur = fs::read_to_string(path).ok();
    if cur.as_deref() != Some(contents) {
        fs::write(path, contents).unwrap_or_else(|e| {
            panic!("write {}: {}", path.display(), e);
        });
    }
}

#[derive(Debug, Clone)]
struct AbiFn {
    name: String,
    params: Vec<(String, String)>, // (param_name, rust_type)
    ret: Option<String>,           // None == void
    /// `@since X.Y.Z` doc comment above the attribute, or `None` if
    /// the fn was added without one. The drift gate fails when this
    /// is `None`.
    since: Option<String>,
    /// `@deprecated X.Y.Z[ — note]` doc comment above the attribute.
    /// `(version, optional_note)`.
    deprecated: Option<(String, Option<String>)>,
}

/// Walk `codegen_abi.rs` and pull out every `#[no_mangle] pub extern
/// "C" fn mty_runtime_*` signature. The parser is deliberately simple
/// (line-oriented + brace-balanced collection of the signature) — the
/// file follows a stable hand-written shape that this only needs to
/// keep up with. If the convention ever changes (e.g. someone wraps a
/// no_mangle fn in cfg-gate macros), the `runtime_abi_header_in_sync`
/// test will fail because the symbol_table() vs parser disagree.
///
/// v0.47 T3: the parser also walks BACKWARDS from `#[no_mangle]` to
/// pick up `// @since X.Y.Z` and `// @deprecated X.Y.Z[ — note]`
/// markers in the contiguous block of `//`-line-comments immediately
/// preceding the attribute. Blank lines / non-comment lines break
/// the block so a comment from an unrelated earlier fn doesn't bleed
/// down.
fn parse_abi(src: &str) -> Vec<AbiFn> {
    let mut out = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_start();
        if line == "#[no_mangle]" {
            // Walk backwards from i-1 to collect the contiguous
            // `//`-comment block immediately above the attribute.
            let (since, deprecated) = collect_markers(&lines, i);

            // Collect lines until we see `{` (body start). Strip
            // trailing comments by collapsing whitespace and stopping
            // at `{`.
            let mut j = i + 1;
            let mut sig = String::new();
            while j < lines.len() {
                sig.push_str(lines[j]);
                sig.push(' ');
                if lines[j].contains('{') {
                    break;
                }
                j += 1;
            }
            if let Some(mut parsed) = parse_extern_c_fn(&sig) {
                if parsed.name.starts_with("mty_runtime_") {
                    parsed.since = since;
                    parsed.deprecated = deprecated;
                    out.push(parsed);
                }
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// Walk backwards from the `#[no_mangle]` line at index `attr_idx`
/// over the contiguous block of `//`-line comments and pick up the
/// last `@since` / `@deprecated` marker. The block stops at the
/// first non-comment, non-blank-line-attached-comment row — blank
/// lines also break it. We deliberately only honor `//` comments
/// (not `///` doc comments or `/* */` block comments) so the
/// markers stay obviously distinct from real doc attributes.
fn collect_markers(
    lines: &[&str],
    attr_idx: usize,
) -> (Option<String>, Option<(String, Option<String>)>) {
    let mut since: Option<String> = None;
    let mut deprecated: Option<(String, Option<String>)> = None;
    if attr_idx == 0 {
        return (since, deprecated);
    }
    let mut k = attr_idx;
    while k > 0 {
        k -= 1;
        let trimmed = lines[k].trim_start();
        // Stop at the first non-comment line. Doc comments (`///`)
        // and block comments are NOT honored here — only plain `//`.
        if !trimmed.starts_with("//") || trimmed.starts_with("///") {
            break;
        }
        // Strip leading `//` and any leading whitespace inside.
        let payload = trimmed.trim_start_matches("//").trim();
        if let Some(rest) = payload.strip_prefix("@since ") {
            if since.is_none() {
                since = Some(rest.trim().to_string());
            }
        } else if let Some(rest) = payload.strip_prefix("@deprecated ") {
            if deprecated.is_none() {
                // Either `0.47.0` or `0.47.0 — use X` / `0.47.0 - use X`.
                let rest = rest.trim();
                let (ver, note) = split_deprecated_payload(rest);
                deprecated = Some((ver, note));
            }
        }
    }
    (since, deprecated)
}

/// Split `"0.47.0 — use mty_runtime_fs_dir_open"` into
/// `("0.47.0", Some("use mty_runtime_fs_dir_open"))`. The separator
/// can be a literal em-dash, an ASCII `-`, or `--`. If there is no
/// separator, the whole string is treated as the version.
fn split_deprecated_payload(s: &str) -> (String, Option<String>) {
    // Look for em-dash first, then ` -- `, then ` - `.
    for sep in ["—", " -- ", " - "] {
        if let Some(idx) = s.find(sep) {
            let ver = s[..idx].trim().to_string();
            let note = s[idx + sep.len()..].trim().to_string();
            return (ver, if note.is_empty() { None } else { Some(note) });
        }
    }
    (s.trim().to_string(), None)
}

fn parse_extern_c_fn(raw: &str) -> Option<AbiFn> {
    // Expect: `pub extern "C" fn <name>(<params>) [-> <ret>] {`.
    let trimmed = raw.trim();
    // Locate the body-open brace; we only need everything up to (and
    // including) it. Take what's BEFORE the brace as the signature.
    let brace = trimmed.find('{')?;
    let sig = trimmed[..brace].trim();

    let after = sig.strip_prefix("pub extern \"C\" fn ")?;
    let lparen = after.find('(')?;
    let name = after[..lparen].trim().to_string();

    // Find the matching closing paren for the param list.
    let rest = &after[lparen + 1..];
    let mut depth = 1usize;
    let mut end = None;
    for (idx, ch) in rest.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(idx);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    let params_src = rest[..end].trim();
    let tail = rest[end + 1..].trim();

    let ret = tail
        .strip_prefix("->")
        .map(|arrow_rest| arrow_rest.trim().trim_end_matches(',').trim().to_string());

    let params = split_params(params_src);
    Some(AbiFn {
        name,
        params,
        ret,
        since: None,
        deprecated: None,
    })
}

fn split_params(s: &str) -> Vec<(String, String)> {
    let s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut chunks: Vec<&str> = Vec::new();
    for (idx, ch) in s.char_indices() {
        match ch {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                chunks.push(&s[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    chunks.push(&s[start..]);
    chunks
        .into_iter()
        .filter_map(|chunk| {
            let chunk = chunk.trim();
            if chunk.is_empty() {
                return None;
            }
            let colon = chunk.find(':')?;
            let name = chunk[..colon].trim().trim_start_matches('_').to_string();
            let ty = chunk[colon + 1..].trim().to_string();
            Some((name, ty))
        })
        .collect()
}

fn rust_to_c(ty: &str) -> &'static str {
    match ty.trim() {
        "i8" => "int8_t",
        "i16" => "int16_t",
        "i32" => "int32_t",
        "i64" => "int64_t",
        "u8" => "uint8_t",
        "u16" => "uint16_t",
        "u32" => "uint32_t",
        "u64" => "uint64_t",
        "f32" => "float",
        "f64" => "double",
        // bool isn't currently in the surface but include for safety.
        "bool" => "bool",
        // Unrecognized — fall back to int64_t (matches the codegen's
        // pun convention) and surface a build warning so we notice.
        other => {
            println!(
                "cargo:warning=unknown Rust ABI type `{other}` — \
                 defaulting to int64_t in mty_runtime_abi.h"
            );
            "int64_t"
        }
    }
}

fn render_header(fns: &[AbiFn], version: &str) -> String {
    let (major, minor, patch) = split_version(version);
    let mut s = String::new();
    s.push_str("/* GENERATED by mty-runtime/build.rs — DO NOT EDIT BY HAND.\n");
    s.push_str(" *\n");
    s.push_str(" * Mighty runtime ABI — official C declarations for every\n");
    s.push_str(" * `mty_runtime_*` C-ABI symbol the compiler may emit calls\n");
    s.push_str(" * to from JIT'd or AOT-compiled Mighty code.\n");
    s.push_str(" *\n");
    s.push_str(" * v0.46 T1 first ship: see docs/internals/runtime-abi.md for\n");
    s.push_str(" * the stability story and `mty abi list` for the symbol\n");
    s.push_str(" * table at runtime.\n");
    s.push_str(" *\n");
    s.push_str(" * v0.47 T3: numeric version macros\n");
    s.push_str(" * (MTY_RUNTIME_ABI_VERSION_MAJOR/MINOR/PATCH) plus per-fn\n");
    s.push_str(" * `@since X.Y.Z` (and where applicable `@deprecated X.Y.Z`)\n");
    s.push_str(" * markers so downstream consumers can soft-pin with\n");
    s.push_str(" *   `#if MTY_RUNTIME_ABI_VERSION_MINOR >= 47`\n");
    s.push_str(" * and see API age at a glance.\n");
    s.push_str(" *\n");
    s.push_str(" * Consumers link against `libmty_runtime_abi.a`\n");
    s.push_str(" * (Linux/macOS) or `mty_runtime_abi.lib` (MSVC). See the\n");
    s.push_str(" * docs for the exact tarball name per platform.\n");
    s.push_str(" */\n");
    s.push_str("#ifndef MTY_RUNTIME_ABI_H\n");
    s.push_str("#define MTY_RUNTIME_ABI_H\n\n");
    s.push_str("#include <stdint.h>\n\n");
    s.push_str(&format!("#define MTY_RUNTIME_ABI_VERSION \"{version}\"\n"));
    s.push_str(&format!("#define MTY_RUNTIME_ABI_VERSION_MAJOR {major}\n"));
    s.push_str(&format!("#define MTY_RUNTIME_ABI_VERSION_MINOR {minor}\n"));
    s.push_str(&format!("#define MTY_RUNTIME_ABI_VERSION_PATCH {patch}\n"));
    // v0.47 T3 — a single encoded integer so consumers can do
    // `#if MTY_RUNTIME_ABI_VERSION_NUMBER >= 4700` style compile-time
    // comparisons against one value instead of three separate macros.
    // Encoding: MAJOR*10000 + MINOR*100 + PATCH (room for 99 minor /
    // 99 patch per major, matching the project's 0.x cadence).
    let number = major * 10000 + minor * 100 + patch;
    s.push_str(&format!(
        "#define MTY_RUNTIME_ABI_VERSION_NUMBER {number} \
         /* MAJOR*10000 + MINOR*100 + PATCH */\n"
    ));
    // v0.47 T3 — header-level stability marker. The whole runtime ABI
    // is pre-1.0 / experimental: symbols may be added, deprecated, or
    // reshaped between minor releases. Downstream consumers can branch
    // on this string (or just surface it in diagnostics) to make the
    // pre-1.0 contract explicit.
    s.push_str(&format!(
        "#define MTY_RUNTIME_ABI_STABILITY \"{ABI_STABILITY}\"\n\n"
    ));
    s.push_str("#ifdef __cplusplus\n");
    s.push_str("extern \"C\" {\n");
    s.push_str("#endif\n\n");
    for f in fns {
        // v0.47 T3 — render `/* @since … */` / `/* @deprecated … */`
        // comment above each declaration if we picked one up from
        // codegen_abi.rs. We combine both onto one line when both
        // are present to keep the header skimmable.
        if f.since.is_some() || f.deprecated.is_some() {
            let mut markers = String::new();
            if let Some(s) = &f.since {
                markers.push_str(&format!("@since {s}"));
            }
            if let Some((ver, note)) = &f.deprecated {
                if !markers.is_empty() {
                    markers.push(' ');
                }
                match note {
                    Some(n) => markers.push_str(&format!("@deprecated {ver} — {n}")),
                    None => markers.push_str(&format!("@deprecated {ver}")),
                }
            }
            s.push_str(&format!("/* {markers} */\n"));
        }
        let ret = match &f.ret {
            None => "void".to_string(),
            Some(rty) => rust_to_c(rty).to_string(),
        };
        let params = if f.params.is_empty() {
            "void".to_string()
        } else {
            f.params
                .iter()
                .map(|(n, t)| format!("{} {}", rust_to_c(t), n))
                .collect::<Vec<_>>()
                .join(", ")
        };
        s.push_str(&format!("{ret} {}({});\n", f.name, params));
    }
    s.push_str("\n#ifdef __cplusplus\n");
    s.push_str("} /* extern \"C\" */\n");
    s.push_str("#endif\n\n");
    s.push_str("#endif /* MTY_RUNTIME_ABI_H */\n");
    s
}

fn render_symbol_table(fns: &[AbiFn], version: &str) -> String {
    let (major, minor, patch) = split_version(version);
    let mut s = String::new();
    s.push_str("// GENERATED by mty-runtime/build.rs — DO NOT EDIT BY HAND.\n");
    s.push_str("// Consumed by `crate::abi_export`. See build.rs for shape.\n\n");
    s.push_str(&format!(
        "pub const RUNTIME_ABI_VERSION: &str = \"{version}\";\n"
    ));
    s.push_str(&format!(
        "pub const RUNTIME_ABI_VERSION_MAJOR: u32 = {major};\n"
    ));
    s.push_str(&format!(
        "pub const RUNTIME_ABI_VERSION_MINOR: u32 = {minor};\n"
    ));
    s.push_str(&format!(
        "pub const RUNTIME_ABI_VERSION_PATCH: u32 = {patch};\n"
    ));
    // v0.47 T3 — encoded combined value mirroring the C
    // `MTY_RUNTIME_ABI_VERSION_NUMBER` macro.
    let number = major * 10000 + minor * 100 + patch;
    s.push_str(&format!(
        "pub const RUNTIME_ABI_VERSION_NUMBER: u32 = {number};\n"
    ));
    // v0.47 T3 — header-level stability marker, mirrors the C
    // `MTY_RUNTIME_ABI_STABILITY` macro.
    s.push_str(&format!(
        "pub const RUNTIME_ABI_STABILITY: &str = \"{ABI_STABILITY}\";\n\n"
    ));
    s.push_str("pub static RUNTIME_ABI_SIGNATURES: &[AbiSignature] = &[\n");
    for f in fns {
        let ret = f.ret.as_deref().unwrap_or("()");
        let params: Vec<String> = f
            .params
            .iter()
            .map(|(n, t)| format!("(\"{n}\", \"{t}\")"))
            .collect();
        let since = match &f.since {
            Some(v) => format!("Some(\"{v}\")"),
            None => "None".to_string(),
        };
        let deprecated = match &f.deprecated {
            Some((v, Some(n))) => format!(
                "Some(AbiDeprecation {{ since: \"{v}\", note: Some(\"{}\") }})",
                escape_rust_str_literal(n)
            ),
            Some((v, None)) => {
                format!("Some(AbiDeprecation {{ since: \"{v}\", note: None }})")
            }
            None => "None".to_string(),
        };
        s.push_str(&format!(
            "    AbiSignature {{ name: \"{}\", params: &[{}], ret: \"{}\", since: {}, deprecated: {} }},\n",
            f.name,
            params.join(", "),
            ret,
            since,
            deprecated
        ));
    }
    s.push_str("];\n");
    s
}

/// Escape a string for embedding into a Rust string literal in the
/// generated symbol-table side file. We only need to handle `"` and
/// `\\` — none of the existing notes contain any other troublesome
/// chars, but we escape them anyway so future additions don't bite.
fn escape_rust_str_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            other => out.push(other),
        }
    }
    out
}
