// v0.46 T1 — Runtime ABI artifact pipeline.
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
// The check-in copy under `crates/mty-runtime/include/` is the
// source-of-truth artifact (so users can `git diff` it across
// releases). The build script re-emits it on every build; if a track
// adds a new `#[no_mangle]` fn without committing the regenerated
// header, the build STILL succeeds and updates the file on disk —
// but our `runtime_abi_header_in_sync` test (see
// `tests/runtime_abi_header.rs`) then fails because the checked-in
// copy on `git` lags the generator output. This makes drift visible
// without breaking incremental dev.

use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env_or_panic("CARGO_MANIFEST_DIR"));
    let abi_src = manifest_dir.join("src").join("codegen_abi.rs");
    println!("cargo:rerun-if-changed={}", abi_src.display());
    println!("cargo:rerun-if-changed=build.rs");

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
}

/// Walk `codegen_abi.rs` and pull out every `#[no_mangle] pub extern
/// "C" fn mty_runtime_*` signature. The parser is deliberately simple
/// (line-oriented + brace-balanced collection of the signature) — the
/// file follows a stable hand-written shape that this only needs to
/// keep up with. If the convention ever changes (e.g. someone wraps a
/// no_mangle fn in cfg-gate macros), the `runtime_abi_header_in_sync`
/// test will fail because the symbol_table() vs parser disagree.
fn parse_abi(src: &str) -> Vec<AbiFn> {
    let mut out = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_start();
        if line == "#[no_mangle]" {
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
            if let Some(parsed) = parse_extern_c_fn(&sig) {
                if parsed.name.starts_with("mty_runtime_") {
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
    Some(AbiFn { name, params, ret })
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
    s.push_str(" * Consumers link against `libmty_runtime_abi.a`\n");
    s.push_str(" * (Linux/macOS) or `mty_runtime_abi.lib` (MSVC). See the\n");
    s.push_str(" * docs for the exact tarball name per platform.\n");
    s.push_str(" */\n");
    s.push_str("#ifndef MTY_RUNTIME_ABI_H\n");
    s.push_str("#define MTY_RUNTIME_ABI_H\n\n");
    s.push_str("#include <stdint.h>\n\n");
    s.push_str(&format!(
        "#define MTY_RUNTIME_ABI_VERSION \"{version}\"\n\n"
    ));
    s.push_str("#ifdef __cplusplus\n");
    s.push_str("extern \"C\" {\n");
    s.push_str("#endif\n\n");
    for f in fns {
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
    let mut s = String::new();
    s.push_str("// GENERATED by mty-runtime/build.rs — DO NOT EDIT BY HAND.\n");
    s.push_str("// Consumed by `crate::abi_export`. See build.rs for shape.\n\n");
    s.push_str(&format!(
        "pub const RUNTIME_ABI_VERSION: &str = \"{version}\";\n\n"
    ));
    s.push_str("pub static RUNTIME_ABI_SIGNATURES: &[AbiSignature] = &[\n");
    for f in fns {
        let ret = f.ret.as_deref().unwrap_or("()");
        let params: Vec<String> = f
            .params
            .iter()
            .map(|(n, t)| format!("(\"{n}\", \"{t}\")"))
            .collect();
        s.push_str(&format!(
            "    AbiSignature {{ name: \"{}\", params: &[{}], ret: \"{}\" }},\n",
            f.name,
            params.join(", "),
            ret
        ));
    }
    s.push_str("];\n");
    s
}
