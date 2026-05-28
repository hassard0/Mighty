//! `mty find <query>` — capability-tagged stdlib search (v0.33 T7).
//!
//! Lets agents (and humans) describe an intent in natural language and
//! get pointed at the closest Mighty stdlib surface. The index is
//! constructed by walking `crates/mty-stdlib/src/**/*.rs` and parsing
//! out `pub fn`/`pub struct`/`pub enum`/`pub trait`/`pub const`/`pub type`
//! items together with their preceding `///` doc comment. Each item
//! gets:
//!
//!   * a module path (e.g. `std.fs`, `std.memory.vector`),
//!   * a synthesized one-line signature,
//!   * a verb set (extracted from name + doc tokens),
//!   * a capability tag (heuristic, anchored on the module path + doc),
//!   * the first example block embedded in the doc comment.
//!
//! The result is persisted at `~/.mty/find-index.json` and rebuilt when
//! the stdlib tree's hash changes. Both an interactive table and a
//! machine-readable `--format json` mode are exposed; the latter is the
//! one demo 07's research agent uses to discover APIs without a human
//! in the loop.
//!
//! See `docs/reference/find.md` for the query DSL + ranking spec.
//!
//! # Implementation note — why regex, not syn
//!
//! We deliberately avoid pulling `syn` into the CLI. The stdlib is
//! exclusively normal-shape `pub fn`/`pub struct`/`impl`/`///` source,
//! so a small state-machine scanner gives us 99% recall with zero
//! extra dependencies and rebuild cost. If the stdlib ever sprouts
//! exotic shapes the doc generator can't already handle, we can lift
//! the parse to `mty_doc` (which already has the heavy machinery).
//!
//! All public surfaces are tested in `tests/cmd_find.rs`.
//!
//! NOTE: this module is intentionally self-contained — no
//! `mty-stdlib` / `mty-syntax` dependency added.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------
// Public CLI entry
// ---------------------------------------------------------------------

/// Argument shape for `mty find`.
///
/// `query` is positional — at least one of `query` / `by_capability`
/// must be set. `format` is one of `"pretty"` / `"json"` / `"short"`.
/// `--explain` toggles a per-result capability + minimal-example block.
#[derive(Debug, Clone)]
pub struct FindArgs {
    pub query: Option<String>,
    pub by_capability: Option<String>,
    pub format: String,
    pub explain: bool,
    pub top: usize,
    /// Force a fresh index rebuild instead of reading the cache.
    pub rebuild: bool,
    /// Override the stdlib source root (mostly for tests).
    pub stdlib_root: Option<PathBuf>,
    /// Override the on-disk index cache path (mostly for tests).
    pub index_path: Option<PathBuf>,
}

impl Default for FindArgs {
    fn default() -> Self {
        Self {
            query: None,
            by_capability: None,
            format: "pretty".to_string(),
            explain: false,
            top: 5,
            rebuild: false,
            stdlib_root: None,
            index_path: None,
        }
    }
}

/// `mty find` entry. Returns the CLI exit code.
pub fn run(args: FindArgs) -> i32 {
    let Some(stdlib_root) = resolve_stdlib_root(args.stdlib_root.clone()) else {
        eprintln!(
            "mty find: could not locate the mty-stdlib source tree.\n\
             Set MTY_STDLIB_ROOT or run from inside the Mighty workspace."
        );
        return 1;
    };

    let index_path = args
        .index_path
        .clone()
        .unwrap_or_else(default_index_cache_path);

    let index = match load_or_build_index(&stdlib_root, &index_path, args.rebuild) {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("mty find: failed to build index: {e}");
            return 1;
        }
    };

    if let Some(cap) = args.by_capability.as_ref() {
        return print_by_capability(&index, cap, &args);
    }

    let Some(query) = args.query.as_ref() else {
        eprintln!(
            "mty find: pass a query (e.g. `mty find \"write files\"`) or `--by-capability fs.write`."
        );
        return 2;
    };

    let hits = rank(&index, query, args.top.max(1));
    print_hits(&hits, &args);
    0
}

// ---------------------------------------------------------------------
// Index model
// ---------------------------------------------------------------------

/// One indexed public item. The `score` field is filled in by [`rank`]
/// — it's `0.0` on the persisted on-disk shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub name: String,
    /// Mighty-style module path, e.g. `std.fs` or `std.memory.vector`.
    pub module: String,
    /// One of `fn`, `struct`, `enum`, `trait`, `const`, `type`.
    pub kind: String,
    /// One-line synthesized signature, e.g. `fn read(cap: &FsCap, path: &Path) -> Result<Vec<u8>, IoErr>`.
    pub signature: String,
    /// Capability tag (best-effort, may be empty).
    pub capability: String,
    /// Tokens extracted from name + doc, used for verb-match ranking.
    pub verbs: BTreeSet<String>,
    /// First doc-comment paragraph (terse).
    pub summary: String,
    /// First fenced code block in the doc, if any.
    pub example: Option<String>,
    /// Source file (relative to the stdlib root) + line for debugging.
    pub source: String,
    /// Search score. Not persisted to disk; recomputed per query.
    #[serde(skip)]
    pub score: f32,
}

/// On-disk shape. `stdlib_hash` lets us invalidate the cache when the
/// stdlib source tree changes (cheap: a sha256 of sorted file mtimes +
/// sizes; we don't need cryptographic strength here).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub version: u32,
    pub stdlib_hash: String,
    pub items: Vec<Item>,
}

const INDEX_VERSION: u32 = 1;

impl Index {
    pub fn empty() -> Self {
        Self {
            version: INDEX_VERSION,
            stdlib_hash: String::new(),
            items: Vec::new(),
        }
    }

    pub fn capabilities(&self) -> BTreeSet<String> {
        self.items
            .iter()
            .map(|i| i.capability.clone())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

// ---------------------------------------------------------------------
// Cache + rebuild glue
// ---------------------------------------------------------------------

fn default_index_cache_path() -> PathBuf {
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".mty").join("find-index.json")
}

/// Locate the stdlib source root. Order of precedence:
///   1. explicit override from `FindArgs.stdlib_root`,
///   2. `$MTY_STDLIB_ROOT`,
///   3. walk up from the current working directory looking for
///      `crates/mty-stdlib/src/lib.rs`,
///   4. walk up from `std::env::current_exe()` (handles `mty` running
///      from inside a Cargo target dir).
fn resolve_stdlib_root(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        if p.join("lib.rs").exists() {
            return Some(p);
        }
    }
    if let Ok(v) = std::env::var("MTY_STDLIB_ROOT") {
        let p = PathBuf::from(v);
        if p.join("lib.rs").exists() {
            return Some(p);
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(p) = walk_up_for_stdlib(&cwd) {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if let Some(p) = walk_up_for_stdlib(parent) {
                return Some(p);
            }
        }
    }
    None
}

fn walk_up_for_stdlib(start: &Path) -> Option<PathBuf> {
    let mut cur: PathBuf = start.to_path_buf();
    loop {
        let candidate = cur.join("crates").join("mty-stdlib").join("src");
        if candidate.join("lib.rs").exists() {
            return Some(candidate);
        }
        if !cur.pop() {
            return None;
        }
    }
}

fn load_or_build_index(
    stdlib_root: &Path,
    cache_path: &Path,
    force_rebuild: bool,
) -> std::io::Result<Index> {
    let want_hash = hash_stdlib(stdlib_root);
    if !force_rebuild {
        if let Ok(bytes) = std::fs::read(cache_path) {
            if let Ok(idx) = serde_json::from_slice::<Index>(&bytes) {
                if idx.version == INDEX_VERSION && idx.stdlib_hash == want_hash {
                    return Ok(idx);
                }
            }
        }
    }
    let items = walk_stdlib(stdlib_root);
    let idx = Index {
        version: INDEX_VERSION,
        stdlib_hash: want_hash,
        items,
    };
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Best-effort persist — we don't fail the user-facing search if
    // disk is read-only.
    let _ = serde_json::to_vec_pretty(&idx)
        .map_err(std::io::Error::other)
        .and_then(|b| std::fs::write(cache_path, b));
    Ok(idx)
}

fn hash_stdlib(stdlib_root: &Path) -> String {
    // Cheap content-derived hash: sort all *.rs files by relative path,
    // mix in (size, mtime nanos). Not cryptographic — we just need to
    // detect "stdlib changed since cache was written".
    let mut tuples: Vec<(String, u64, u128)> = Vec::new();
    for path in collect_rs_files(stdlib_root) {
        let rel = path
            .strip_prefix(stdlib_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if let Ok(meta) = std::fs::metadata(&path) {
            let size = meta.len();
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            tuples.push((rel, size, mtime));
        }
    }
    tuples.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a offset
    for (rel, size, mtime) in &tuples {
        for byte in rel.as_bytes() {
            h ^= u64::from(*byte);
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= *size;
        h = h.wrapping_mul(0x100000001b3);
        h ^= *mtime as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{h:016x}")
}

fn collect_rs_files(root: &Path) -> Vec<PathBuf> {
    fn walk(p: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(p) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Skip the `bin/` directory — it's the CLI test runner,
                // not a stdlib surface users want to discover.
                if path.file_name().and_then(|s| s.to_str()) == Some("bin") {
                    continue;
                }
                walk(&path, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

// ---------------------------------------------------------------------
// Source walker — extracts public items
// ---------------------------------------------------------------------

fn walk_stdlib(stdlib_root: &Path) -> Vec<Item> {
    let mut items = Vec::new();
    for path in collect_rs_files(stdlib_root) {
        if let Ok(src) = std::fs::read_to_string(&path) {
            let module = derive_module_path(stdlib_root, &path);
            items.extend(extract_items(&src, &module, &relative(stdlib_root, &path)));
        }
    }
    items
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Convert a path inside the stdlib tree into a Mighty-style module
/// path (`std.fs`, `std.memory.vector`, …). `mod.rs` collapses onto its
/// parent directory; `lib.rs` becomes `std`.
pub(crate) fn derive_module_path(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let mut parts: Vec<String> = Vec::new();
    for comp in rel.components() {
        if let std::path::Component::Normal(os) = comp {
            if let Some(s) = os.to_str() {
                parts.push(s.to_string());
            }
        }
    }
    // Strip the leading `src/` if present (callers pass `…/src/`).
    if parts.first().map(String::as_str) == Some("src") {
        parts.remove(0);
    }
    // Collapse the file segment.
    let last = parts.pop().unwrap_or_default();
    let file_stem = last.trim_end_matches(".rs");
    match file_stem {
        "lib" => {
            let mut s = "std".to_string();
            for p in &parts {
                s.push('.');
                s.push_str(p);
            }
            s
        }
        "mod" => {
            let mut s = "std".to_string();
            for p in &parts {
                s.push('.');
                s.push_str(p);
            }
            s
        }
        other => {
            let mut s = "std".to_string();
            for p in &parts {
                s.push('.');
                s.push_str(p);
            }
            s.push('.');
            s.push_str(other);
            s
        }
    }
}

/// Walk source text and extract every `pub` item with its preceding
/// `///` doc comment. We're after coverage, not perfection — items
/// inside `impl` blocks (the `VectorStore::local` / `Working::stage`
/// shape) are picked up too, prefixed with the `impl` target's name.
pub(crate) fn extract_items(src: &str, module: &str, source_rel: &str) -> Vec<Item> {
    let mut out = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    // Track current `impl` stack so methods get the right module path.
    let mut impl_stack: Vec<String> = Vec::new();
    let mut brace_depth_at_impl: Vec<i32> = Vec::new();
    let mut brace_depth: i32 = 0;

    let mut i = 0usize;
    while i < lines.len() {
        let raw = lines[i];
        let line = raw.trim_start();

        // Track `impl <Foo> {` blocks so we know we're inside one when
        // we hit a method. We strip generics and trait clauses to get
        // a clean type name.
        if (line.starts_with("impl ") || line.starts_with("impl<"))
            && line.contains('{')
        {
            let ty = parse_impl_target(line);
            impl_stack.push(ty);
            brace_depth_at_impl.push(brace_depth);
            brace_depth += count_brace_delta(raw);
            i += 1;
            continue;
        }

        // Update brace depth + pop `impl` when its block closes.
        brace_depth += count_brace_delta(raw);
        while let Some(depth) = brace_depth_at_impl.last() {
            if brace_depth <= *depth {
                brace_depth_at_impl.pop();
                impl_stack.pop();
            } else {
                break;
            }
        }

        // Pre-collect any doc lines.
        let mut doc_buf: Vec<String> = Vec::new();
        let mut j = i;
        while j > 0 {
            let prev = lines[j - 1].trim_start();
            if prev.starts_with("///") {
                let body = prev.trim_start_matches('/').trim_start_matches(' ');
                doc_buf.push(body.to_string());
                j -= 1;
            } else if prev.starts_with("//!")
                || prev.starts_with("#[")
                || prev.starts_with("#![")
                || prev.is_empty()
            {
                // skip attrs / blank lines while still scanning back
                j -= 1;
            } else {
                break;
            }
        }
        doc_buf.reverse();
        let doc = doc_buf.join("\n");

        // Detect a public item on this line.
        let item_line = lines[i];
        let trimmed = item_line.trim_start();
        if let Some(kind) = detect_pub_item_kind(trimmed) {
            // For functions, capture the full signature across multiple
            // lines (until `{` or `;`).
            let (sig, sig_end) = collect_signature(&lines, i);
            let name = parse_item_name(kind, &sig).unwrap_or_default();
            if !name.is_empty() {
                let scope_module = if impl_stack.is_empty() {
                    module.to_string()
                } else {
                    format!("{}.{}", module, impl_stack.last().unwrap())
                };
                let summary = first_paragraph(&doc);
                let example = first_example_block(&doc);
                let capability = infer_capability(&scope_module, &name, &doc);
                let verbs = extract_verbs(&name, &doc);
                let signature = synthesize_signature(kind, &sig);
                let source_str = format!("{}:{}", source_rel, i + 1);
                out.push(Item {
                    name,
                    module: scope_module,
                    kind: kind.to_string(),
                    signature,
                    capability,
                    verbs,
                    summary,
                    example,
                    source: source_str,
                    score: 0.0,
                });
            }
            i = sig_end + 1;
            continue;
        }

        i += 1;
    }
    out
}

fn count_brace_delta(s: &str) -> i32 {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut prev = ' ';
    for c in s.chars() {
        if c == '"' && prev != '\\' {
            in_str = !in_str;
        }
        if !in_str {
            if c == '{' {
                depth += 1;
            }
            if c == '}' {
                depth -= 1;
            }
        }
        prev = c;
    }
    depth
}

fn parse_impl_target(line: &str) -> String {
    // Strip leading `impl<…>` and trait clause, return type name.
    let rest = line.trim_start_matches("impl");
    let rest = rest.trim_start();
    // Skip generic params if present.
    let rest = if rest.starts_with('<') {
        let mut depth = 0;
        let mut end = 0;
        for (k, c) in rest.char_indices() {
            if c == '<' {
                depth += 1;
            }
            if c == '>' {
                depth -= 1;
                if depth == 0 {
                    end = k + c.len_utf8();
                    break;
                }
            }
        }
        rest[end..].trim_start()
    } else {
        rest
    };
    // `Trait for Type { … }` → take after `for`.
    let candidate = if let Some(idx) = rest.find(" for ") {
        &rest[idx + 5..]
    } else {
        rest
    };
    let candidate = candidate.trim();
    // Take up to `<` / ` ` / `{` / `:` to get the bare type name.
    let mut name = String::new();
    for c in candidate.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            name.push(c);
        } else {
            break;
        }
    }
    name
}

fn detect_pub_item_kind(s: &str) -> Option<&'static str> {
    // We only index `pub` items — private fns inside `#[cfg(test)]`
    // blocks would otherwise leak into the index and pollute the
    // top-N rankings. Accept `pub`, `pub(crate)`, and `pub(super)`.
    let after_pub = s
        .strip_prefix("pub(crate) ")
        .or_else(|| s.strip_prefix("pub(super) "))
        .or_else(|| s.strip_prefix("pub "))?;
    // We accept `async fn`, `const fn`, `unsafe fn`, `extern fn`.
    let s = after_pub
        .trim_start_matches("async ")
        .trim_start_matches("const ")
        .trim_start_matches("unsafe ")
        .trim_start_matches("extern ");
    if s.starts_with("fn ") {
        Some("fn")
    } else if s.starts_with("struct ") {
        Some("struct")
    } else if s.starts_with("enum ") {
        Some("enum")
    } else if s.starts_with("trait ") {
        Some("trait")
    } else if s.starts_with("const ") {
        Some("const")
    } else if s.starts_with("type ") {
        Some("type")
    } else {
        None
    }
}

fn collect_signature(lines: &[&str], start: usize) -> (String, usize) {
    let mut buf = String::new();
    let mut depth_angle = 0i32;
    let mut i = start;
    let mut end = start;
    let stop_on_semi = matches!(
        detect_pub_item_kind(lines[start].trim_start()),
        Some("const" | "type")
    );
    while i < lines.len() {
        let line = lines[i];
        buf.push_str(line);
        buf.push(' ');
        end = i;
        for c in line.chars() {
            if c == '<' {
                depth_angle += 1;
            }
            if c == '>' && depth_angle > 0 {
                depth_angle -= 1;
            }
        }
        if stop_on_semi && line.contains(';') {
            break;
        }
        if !stop_on_semi && depth_angle == 0 && (line.contains('{') || line.contains(';')) {
            break;
        }
        i += 1;
    }
    (buf, end)
}

fn parse_item_name(kind: &str, sig: &str) -> Option<String> {
    // For each item kind, the name is the first identifier after the
    // kind keyword.
    let after_pub = sig
        .trim_start()
        .trim_start_matches("pub(crate) ")
        .trim_start_matches("pub(super) ")
        .trim_start_matches("pub ")
        .trim_start_matches("async ")
        .trim_start_matches("const ")
        .trim_start_matches("unsafe ")
        .trim_start_matches("extern ");
    let after_kind = match kind {
        "fn" => after_pub.trim_start_matches("fn ").trim_start(),
        "struct" => after_pub.trim_start_matches("struct ").trim_start(),
        "enum" => after_pub.trim_start_matches("enum ").trim_start(),
        "trait" => after_pub.trim_start_matches("trait ").trim_start(),
        "const" => after_pub.trim_start_matches("const ").trim_start(),
        "type" => after_pub.trim_start_matches("type ").trim_start(),
        _ => return None,
    };
    let mut name = String::new();
    for c in after_kind.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            name.push(c);
        } else {
            break;
        }
    }
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn synthesize_signature(kind: &str, sig: &str) -> String {
    // Collapse whitespace, trim trailing `{` / `;`.
    let s: String = sig.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s
        .trim_end_matches(';')
        .trim_end_matches('{')
        .trim()
        .to_string();
    // Prefix with the kind so the table is self-describing.
    match kind {
        "fn" => s,
        _ => s,
    }
}

fn first_paragraph(doc: &str) -> String {
    let mut buf = String::new();
    for line in doc.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !buf.is_empty() {
                break;
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if !buf.is_empty() {
            buf.push(' ');
        }
        buf.push_str(line);
        if buf.len() > 240 {
            break;
        }
    }
    if buf.len() > 240 {
        buf.truncate(237);
        buf.push_str("...");
    }
    buf
}

fn first_example_block(doc: &str) -> Option<String> {
    let mut in_block = false;
    let mut buf = String::new();
    for line in doc.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if in_block {
                return Some(buf.trim_end().to_string());
            }
            in_block = true;
            continue;
        }
        if in_block {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    if buf.is_empty() {
        None
    } else {
        Some(buf.trim_end().to_string())
    }
}

// ---------------------------------------------------------------------
// Capability + verb extraction
// ---------------------------------------------------------------------

/// Heuristic capability inference. We anchor on the module path (the
/// stdlib already groups by capability — `std.fs.*` is `fs.*`,
/// `std.http.*` is `net.https`, etc.) and refine by item name + doc
/// tokens (`read` → `fs.read`, `write` → `fs.write`, `serve` →
/// `net.bind`). Returns an empty string when we can't make a confident
/// guess; consumers should treat that as "untagged".
pub(crate) fn infer_capability(module: &str, name: &str, doc: &str) -> String {
    let n = name.to_ascii_lowercase();
    let d = doc.to_ascii_lowercase();
    // Module-rooted defaults.
    if module.starts_with("std.fs") {
        if n.starts_with("read") || n == "open" || n == "exists" || n == "list_dir" || n == "stat"
        {
            return "fs.read".into();
        }
        if n.starts_with("write")
            || n == "atomic_write"
            || n == "remove"
            || n == "rename"
            || n == "create_dir_all"
        {
            return "fs.write".into();
        }
        return "fs".into();
    }
    if module.starts_with("std.http") || module.starts_with("std.tls") || module.starts_with("std.web") {
        if module.contains("server") || n == "serve" || n.contains("bind") {
            return "net.bind".into();
        }
        return "net.https".into();
    }
    if module.starts_with("std.llm") || module.starts_with("std.swarm") {
        return "net.https + model".into();
    }
    if module.starts_with("std.mcp") {
        return "mcp".into();
    }
    if module.starts_with("std.memory") {
        // VectorStore.qdrant is net; everything else is fs/memory.
        if n.contains("qdrant") || d.contains("qdrant") || d.contains("http") {
            return "net.https".into();
        }
        return "fs.read + fs.write".into();
    }
    if module.starts_with("std.computer") {
        return "computer".into();
    }
    if module.starts_with("std.env") {
        return "env".into();
    }
    if module.starts_with("std.time") {
        return "time".into();
    }
    if module.starts_with("std.random") {
        return "random".into();
    }
    if module.starts_with("std.observe") {
        return "observe".into();
    }
    if module.starts_with("std.eval") || module.starts_with("std.test") {
        return "test".into();
    }
    if module.starts_with("std.log") {
        return "log".into();
    }
    // Doc-token fallbacks for utility modules.
    if d.contains("@tool(cap:") {
        // Extract the explicit cap from the doc.
        if let Some(start) = d.find("@tool(cap:") {
            let rest = &d[start + "@tool(cap:".len()..];
            let end = rest
                .find(')')
                .or_else(|| rest.find(','))
                .unwrap_or(rest.len());
            return rest[..end].trim().trim_matches('"').to_string();
        }
    }
    String::new()
}

/// Tokenize a name + doc into a verb set. We split snake_case +
/// PascalCase + the first ~30 doc tokens, lower-case everything, and
/// drop stop words. The intent is "things a user might type that
/// should match this item."
pub(crate) fn extract_verbs(name: &str, doc: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for tok in split_identifier(name) {
        out.insert(tok);
    }
    for word in doc.split(|c: char| !c.is_ascii_alphanumeric()).take(60) {
        if word.is_empty() {
            continue;
        }
        let w = word.to_ascii_lowercase();
        if STOPWORDS.contains(&w.as_str()) {
            continue;
        }
        if w.len() < 3 {
            continue;
        }
        out.insert(w);
    }
    out
}

fn split_identifier(name: &str) -> Vec<String> {
    // Split on `_` then split each chunk on Camel/Pascal-case
    // boundaries; lower-case everything; drop very short chunks.
    let mut out: Vec<String> = Vec::new();
    for chunk in name.split('_') {
        let mut buf = String::new();
        for (i, c) in chunk.chars().enumerate() {
            if c.is_ascii_uppercase() && i != 0 && !buf.is_empty() {
                out.push(buf.to_ascii_lowercase());
                buf = String::new();
            }
            buf.push(c);
        }
        if !buf.is_empty() {
            out.push(buf.to_ascii_lowercase());
        }
    }
    out.into_iter()
        .filter(|s| s.len() >= 2)
        .collect()
}

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "into", "into", "this", "that", "when", "which", "their",
    "there", "then", "than", "such", "have", "been", "will", "shall", "must", "via", "you", "use",
    "uses", "using", "etc", "see", "note", "also", "non", "any", "all", "but", "not",
];

// ---------------------------------------------------------------------
// Query parsing + ranking
// ---------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ParsedQuery {
    raw: String,
    tokens: Vec<String>,
}

fn parse_query(query: &str) -> ParsedQuery {
    let tokens: Vec<String> = query
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '.')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect();
    ParsedQuery {
        raw: query.to_string(),
        tokens,
    }
}

/// Rank items against a query and return the top `top` by score.
///
/// Ranking (see `docs/reference/find.md`):
/// - exact name match: +1.0
/// - verb match where token == split-name word: +0.9
/// - verb match in doc tokens: +0.7
/// - substring match in name: +0.5
/// - capability match (token is a known capability): +0.6
/// - module-path match (token in module segments): +0.4
/// - "verbatim" exact match in name (case-sensitive): boost +0.2
///
/// Multiple matchers stack; we then sort descending and trim. Ties
/// are broken by module path + name for determinism.
pub fn rank(index: &Index, query: &str, top: usize) -> Vec<Item> {
    let q = parse_query(query);
    let known_caps = index.capabilities();
    let mut scored: Vec<Item> = index
        .items
        .iter()
        .map(|it| {
            let mut s = 0.0f32;
            let name_lc = it.name.to_ascii_lowercase();
            let name_words = split_identifier(&it.name);
            for tok in &q.tokens {
                if tok == &name_lc {
                    s += 1.0;
                }
                if name_words.iter().any(|w| w == tok) {
                    s += 0.9;
                }
                if it.verbs.contains(tok) {
                    s += 0.7;
                }
                if name_lc.contains(tok) && tok != &name_lc {
                    s += 0.5;
                }
                if known_caps.iter().any(|c| c.contains(tok)) && it.capability.contains(tok) {
                    s += 0.6;
                }
                if it
                    .module
                    .split('.')
                    .any(|seg| seg.to_ascii_lowercase() == *tok)
                {
                    s += 0.4;
                }
            }
            // Verbatim exact-case match boost (helps `mty find Tainted`
            // surface the right items first).
            if it.name == q.raw {
                s += 0.2;
            }
            let mut clone = it.clone();
            clone.score = s;
            clone
        })
        .filter(|it| it.score > 0.0)
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.module.cmp(&b.module))
            .then_with(|| a.name.cmp(&b.name))
    });
    scored.truncate(top);
    scored
}

// ---------------------------------------------------------------------
// Output formatters
// ---------------------------------------------------------------------

fn print_hits(hits: &[Item], args: &FindArgs) {
    if hits.is_empty() {
        if args.format == "json" {
            println!("[]");
        } else {
            println!("(no matches)");
        }
        return;
    }
    match args.format.as_str() {
        "json" => print_json(hits),
        "short" => print_short(hits, args.explain),
        _ => print_pretty(hits, args.explain),
    }
}

fn print_pretty(hits: &[Item], explain: bool) {
    println!("{:<28}  {:<26}  {:<5}  score", "ITEM", "MODULE", "KIND");
    println!("{}", "-".repeat(78));
    for it in hits {
        let item_col = truncate(&format!("{}.{}", short_module(&it.module), it.name), 28);
        let module_col = truncate(&it.module, 26);
        println!(
            "{:<28}  {:<26}  {:<5}  {:>5.2}",
            item_col, module_col, it.kind, it.score
        );
        if !it.summary.is_empty() {
            println!("    {}", truncate(&it.summary, 72));
        }
        if explain {
            if !it.capability.is_empty() {
                println!("    cap: {}", it.capability);
            }
            if let Some(ex) = it.example.as_ref() {
                let first_line = ex.lines().next().unwrap_or("");
                if !first_line.is_empty() {
                    println!("    ex:  {}", truncate(first_line, 70));
                }
            }
            println!("    src: {}", it.source);
        }
        println!();
    }
}

fn print_short(hits: &[Item], explain: bool) {
    for it in hits {
        let cap = if it.capability.is_empty() {
            String::new()
        } else {
            format!(" [{}]", it.capability)
        };
        if explain {
            println!(
                "{}.{} ({})  {}{}  -- {}",
                it.module, it.name, it.kind, it.signature, cap, it.source
            );
        } else {
            println!("{}.{} ({}){}", it.module, it.name, it.kind, cap);
        }
    }
}

fn print_json(hits: &[Item]) {
    // NDJSON so agents can stream the output.
    for it in hits {
        if let Ok(s) = serde_json::to_string(it) {
            println!("{s}");
        }
    }
}

fn print_by_capability(index: &Index, cap: &str, args: &FindArgs) -> i32 {
    let needle = cap.trim();
    let mut hits: Vec<Item> = index
        .items
        .iter()
        .filter(|it| {
            it.capability == needle
                || it.capability.split('+').any(|c| c.trim() == needle)
                || it.capability.starts_with(needle)
        })
        .cloned()
        .collect();
    hits.sort_by(|a, b| a.module.cmp(&b.module).then_with(|| a.name.cmp(&b.name)));
    if hits.is_empty() {
        eprintln!(
            "mty find: no items found for capability `{cap}`.\n\
             Known capabilities: {}",
            index
                .capabilities()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ")
        );
        return 1;
    }
    // Set the score field so JSON consumers see 1.0 (a perfect by-cap
    // match) instead of the default 0.0.
    for h in &mut hits {
        h.score = 1.0;
    }
    print_hits(&hits, args);
    0
}

fn short_module(full: &str) -> &str {
    // Strip a leading `std.` to make the table cell narrower.
    full.strip_prefix("std.").unwrap_or(full)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

// ---------------------------------------------------------------------
// Test surface — group all helpers exposed as `pub(crate)` above so
// `tests/cmd_find.rs` can exercise the parser without going through
// the on-disk index.
// ---------------------------------------------------------------------

/// Test-only re-export: walk a single source-string and return the
/// items it would have contributed to the index. Mirrors what
/// [`walk_stdlib`] does for one file.
#[doc(hidden)]
pub fn parse_source_for_tests(src: &str, module: &str, source_rel: &str) -> Vec<Item> {
    extract_items(src, module, source_rel)
}

/// Test-only re-export of the in-memory ranker. `BTreeMap` makes the
/// caller's life easier when asserting per-item scores.
#[doc(hidden)]
pub fn rank_for_tests(items: Vec<Item>, query: &str, top: usize) -> Vec<Item> {
    let idx = Index {
        version: INDEX_VERSION,
        stdlib_hash: String::new(),
        items,
    };
    rank(&idx, query, top)
}

/// Test-only re-export of capability inference.
#[doc(hidden)]
pub fn infer_capability_for_tests(module: &str, name: &str, doc: &str) -> String {
    infer_capability(module, name, doc)
}

/// Test-only re-export of module-path derivation.
#[doc(hidden)]
pub fn derive_module_path_for_tests(root: &Path, path: &Path) -> String {
    derive_module_path(root, path)
}

/// Test-only constructor that round-trips through the on-disk cache.
#[doc(hidden)]
pub fn round_trip_index(items: Vec<Item>) -> Result<Index, String> {
    let idx = Index {
        version: INDEX_VERSION,
        stdlib_hash: "test".into(),
        items,
    };
    let s = serde_json::to_string(&idx).map_err(|e| e.to_string())?;
    let back: Index = serde_json::from_str(&s).map_err(|e| e.to_string())?;
    Ok(back)
}

/// Test-only — surfaces the `BTreeMap`-style summary we'd produce for a
/// `by-capability` listing. Lets the tests assert deterministic
/// ordering without going through stdout.
#[doc(hidden)]
pub fn items_by_capability(index: &Index) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for it in &index.items {
        if it.capability.is_empty() {
            continue;
        }
        out.entry(it.capability.clone())
            .or_default()
            .push(format!("{}.{}", it.module, it.name));
    }
    for v in out.values_mut() {
        v.sort();
    }
    out
}
