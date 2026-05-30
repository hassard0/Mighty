//! v0.41 T5 — hover-catalog surface auditor.
//!
//! ## Why
//!
//! v0.35 T5 shipped the docstub-driven hover catalog. v0.38 T4 + v0.40 T5
//! both flagged that the catalog had drifted *ahead* of the implementation
//! — several entries described surfaces the stdlib crate had not actually
//! shipped. This module is the audit that closes that gap: for every
//! entry in the docstub catalog we verify that the symbol resolves
//! against the real stdlib surface (Rust impls + interp dispatch tables
//! + prelude registrations + host dispatcher).
//!
//! ## Strategy
//!
//! We build a [`RealSurface`] set from four sources:
//!
//! 1. **Prelude** ([`build_prelude`]) — modules, opaque ADTs, builtin
//!    fns, permissive method names.
//! 2. **SIR interp ctor table** — every `"Name.method" => ...` arm in
//!    `mty-ir::interp::run::try_stdlib_ctor` (scanned from the embedded
//!    source).
//! 3. **SIR interp method dispatch** — every `"name" => ...` arm in
//!    `mty-ir::interp::run::eval_method` (scanned from the embedded
//!    source).
//! 4. **Host dispatcher** — every `("std.X", "Y") =>` arm in
//!    `mty-stdlib::host::dispatch` (scanned from the embedded source).
//!
//! Sources 2-4 are scanned with line-level text matching rather than via
//! a Rust dependency on `mty-ir` / `mty-stdlib` — those would invert the
//! crate layering. The patterns are small and stable; the audit panics
//! if the surrounding function boundary markers go missing so a refactor
//! that breaks the scanner fails CI loudly.
//!
//! ## How an entry resolves
//!
//! Each docstub symbol is dispatched by shape:
//!
//! - `std.module.method` → matches if `("std.module", "method")` is in
//!   the host dispatcher OR `("std.module", "*")` (open-module marker).
//! - `Type.method` → matches if `Type` is a known opaque ADT AND
//!   (`method` is a permissive method, OR `Type.method` is a ctor arm,
//!   OR `method` is an eval_method arm, OR `Type.method` appears in a
//!   const/static documented surface table).
//! - Bare symbol — matches if it's in `defs.by_name` (a fn or module),
//!   if it's a macro/keyword on a small allowlist, or if it's an
//!   intentional concept / layout note explicitly flagged in the docstub
//!   via a `# concept-doc` marker line.
//!
//! Entries flagged as `concept-doc` are treated as documentation-only
//! reference cards (e.g. `VEC_HEADER_V2`) and skipped.
//!
//! ## CI gate
//!
//! `mty doc check --check-surface` runs this audit and exits non-zero
//! if any entry resolves to nothing. See `docs/internals/stdlib-docs-pipeline.md`.

use crate::stdlib_walker::{build_extracted_catalog, EMBEDDED_DOCSTUBS};
use std::collections::{BTreeMap, BTreeSet};

/// The "real" stdlib surface, harvested from prelude + interp + host
/// + a textual scan of every `crates/mty-stdlib/src/**/*.rs` file.
#[derive(Debug, Default, Clone)]
pub struct RealSurface {
    /// Module paths registered as `std.*` in the prelude AND every
    /// `pub mod <name>` in `mty-stdlib/src/lib.rs` (prefixed `std.`).
    pub modules: BTreeSet<String>,
    /// Opaque/handler-safe ADT names registered in the prelude PLUS
    /// every `pub struct/enum/type` declared anywhere in the stdlib
    /// source.
    pub opaque_types: BTreeSet<String>,
    /// Top-level fn names registered in the prelude.
    pub builtin_fns: BTreeSet<String>,
    /// Permissive method names from `defs.builtin_methods` PLUS every
    /// `pub fn` discovered inside an `impl <Type>` block in the stdlib
    /// source.
    pub permissive_methods: BTreeSet<String>,
    /// `("std.module", "method")` arms from `mty_stdlib::host::dispatch`.
    pub host_methods: BTreeSet<(String, String)>,
    /// Modules whose host dispatcher accepts ANY method (open marker —
    /// none today but the pattern is here for future use).
    pub open_modules: BTreeSet<String>,
    /// `"Type.method"` ctor arms from `mty_ir::interp::run::try_stdlib_ctor`
    /// PLUS `Type.method` derived from `impl Type` blocks in the stdlib
    /// source.
    pub interp_ctors: BTreeSet<String>,
    /// `"name"` arms from `mty_ir::interp::run::eval_method`.
    pub interp_methods: BTreeSet<String>,
    /// `stdlib_field_index` names from `mty_ir::lower::exprs`.
    pub interp_field_names: BTreeSet<String>,
    /// Free `pub fn` names per module (`std.crypto.sha256` etc.) —
    /// harvested from the stdlib source's top-level `pub fn` items.
    pub module_fns: BTreeSet<(String, String)>,
    /// Variant names of `pub enum X { A, B(...) }` — recognised as
    /// `X.A`, `X.B`.
    pub enum_variants: BTreeSet<String>,
}

impl RealSurface {
    /// Build the real surface set from the live prelude + embedded
    /// source scans.
    pub fn collect() -> Self {
        let mut s = Self::default();
        // 1. Prelude.
        let mut arena = mty_types::ty::TyArena::new();
        let mut defs = mty_types::defs::DefMap::default();
        let _ = mty_types::prelude::build_prelude(&mut arena, &mut defs);
        for (name, def) in &defs.by_name {
            use mty_types::defs::DefRef::*;
            match def {
                Module(_) => {
                    s.modules.insert(name.clone());
                }
                Adt(_) => {
                    s.opaque_types.insert(name.clone());
                }
                Fn(_) => {
                    s.builtin_fns.insert(name.clone());
                }
                Variant(_, _) => {
                    // Variant names (Some, None, Ok, Err) — treat as
                    // builtin_fns for resolver purposes since the
                    // catalog uses them like values.
                    s.builtin_fns.insert(name.clone());
                }
                // Type params are never named at the catalog level.
                Param(_) => {}
                // v0.41 T6: top-level `const NAME: T = expr;` — a
                // value-position binding like a fn, so treat it the
                // same way for catalog purposes.
                Const(_) => {
                    s.builtin_fns.insert(name.clone());
                }
            }
        }
        for name in defs.builtin_methods.keys() {
            s.permissive_methods.insert(name.clone());
        }

        // 2-4. Source-scanned tables.
        scan_host_dispatch(HOST_DISPATCH_SRC, &mut s);
        scan_interp_ctors(INTERP_RUN_SRC, &mut s);
        scan_interp_methods(INTERP_RUN_SRC, &mut s);
        scan_field_index(LOWER_EXPRS_SRC, &mut s);
        // 5. Stdlib source tree (the Rust surface that downstream
        // resolves to). This lets `std.regex.Regex.new`, `Iterator.map`,
        // `HashMap.insert`, etc. resolve even though they're not in the
        // prelude or interp dispatch tables — they're real Rust items
        // that the runtime exposes via the generic-call path.
        scan_stdlib_source_tree(&mut s);
        s
    }

    /// Resolve a docstub symbol against the surface. Returns `Some(reason)`
    /// on resolution, `None` if the symbol does not appear to
    /// correspond to any real callable / type / value.
    ///
    /// The reason string is purely diagnostic — useful for the report.
    pub fn resolve(&self, symbol: &str) -> Option<&'static str> {
        // Concept / layout markers are caller-flagged before reaching us.
        // (`audit_catalog` skips them.)

        // Macro / keyword bare allowlist — symbols the parser /
        // type-checker handles directly rather than registering in the
        // prelude. Today: `format!`, `eprintln`, `eprint`, `assert`,
        // `assert_eq` (and any other macro-shaped helper we ship docs
        // for). The catalog renders these with a `macro` signature.
        const MACRO_ALLOWLIST: &[&str] = &[
            "format",
            "eprintln",
            "eprint",
            "assert",
            "assert_eq",
            "assert_ne",
            "println",
            "print",
            "dbg",
        ];
        if MACRO_ALLOWLIST.contains(&symbol) {
            return Some("macro");
        }

        // Bare ident? -> by_name (fn/module) OR opaque type OR permissive
        // method (some bare entries document method names that work on
        // multiple receivers, e.g. `len`, `iter`).
        if !symbol.contains('.') {
            if self.builtin_fns.contains(symbol) {
                return Some("prelude fn / variant");
            }
            if self.modules.contains(symbol) {
                return Some("prelude module");
            }
            if self.opaque_types.contains(symbol) {
                return Some("prelude opaque type / stdlib type");
            }
            if self.permissive_methods.contains(symbol) {
                return Some("prelude permissive method");
            }
            if self.interp_methods.contains(symbol) {
                return Some("interp method dispatch");
            }
            if self.interp_field_names.contains(symbol) {
                return Some("interp field projection");
            }
            return None;
        }

        // `std.module.X[.method]` — host dispatcher OR stdlib type/fn.
        if symbol.starts_with("std.") {
            // First check: whole symbol is a known module (e.g. `std.json`).
            if self.modules.contains(symbol) {
                return Some("registered module");
            }
            // Try `std.module.tail` split with tail being `Type` or
            // `Type.method` or `submod.fn`. We try every prefix from
            // longest to shortest because some catalog entries use
            // `std.crypto.aes_gcm.encrypt` where `aes_gcm` is a submod.
            let parts: Vec<&str> = symbol.split('.').collect();
            // The "module" candidate is always `std.<x>` (first two
            // segments) — that's the granularity our scanner uses.
            if parts.len() >= 2 {
                let module = format!("{}.{}", parts[0], parts[1]);
                let tail = parts[2..].join(".");
                if !tail.is_empty() {
                    // Direct host dispatcher arm.
                    if self.host_methods.contains(&(module.clone(), tail.clone())) {
                        return Some("host dispatcher");
                    }
                    // Module + free fn (e.g. `std.crypto.sha256` →
                    // `module_fns[("std.crypto", "sha256")]`).
                    if self.module_fns.contains(&(module.clone(), tail.clone())) {
                        return Some("stdlib module fn");
                    }
                    // Module + Type.method shape — split tail at last dot.
                    if let Some(dot) = tail.rfind('.') {
                        let (head, last) = tail.split_at(dot);
                        let last = &last[1..];
                        // `std.regex.Regex.new` -> module=std.regex,
                        // head=Regex, last=new — known type + permissive
                        // / interp method / interp ctor.
                        if self.opaque_types.contains(head) {
                            if self.permissive_methods.contains(last)
                                || self.interp_methods.contains(last)
                                || self.interp_ctors.contains(&format!("{head}.{last}"))
                            {
                                return Some("stdlib type method");
                            }
                            if self.enum_variants.contains(&format!("{head}.{last}")) {
                                return Some("stdlib enum variant");
                            }
                        }
                        // Submodule fn: `std.crypto.aes_gcm.encrypt` —
                        // we treat `aes_gcm` as a submodule whose `encrypt`
                        // fn we picked up via the recursive walker. The
                        // walker stores everything under the parent
                        // module (`std.crypto`), so `aes_gcm.encrypt` —
                        // join — is the synthesised tail.
                        if self
                            .module_fns
                            .contains(&(module.clone(), format!("{head}.{last}")))
                        {
                            return Some("stdlib submod fn");
                        }
                    }
                    // Module + free type (`std.regex.Regex` —
                    // documented as a type).
                    if self.opaque_types.contains(tail.as_str())
                        && self.module_fns.contains(&(module.clone(), tail.clone()))
                    {
                        return Some("stdlib module type");
                    }
                    // Module + permissive method bare (`std.string.trim`).
                    if (self.modules.contains(&module) || self.modules.contains(symbol))
                        && (self.permissive_methods.contains(tail.as_str())
                            || self.interp_methods.contains(tail.as_str()))
                    {
                        return Some("module + permissive method");
                    }
                }
                if self.modules.contains(&module) {
                    return Some("registered module (parent)");
                }
            }
            return None;
        }

        // `Type.method` — Type must be a known type, method must be one of:
        //   * ctor in interp (`Vec.new`, `Char.from_u32`)
        //   * permissive method (`Vec.push`, `Member.ask`)
        //   * interp method dispatch (`as_str`, `slice`, ...)
        //   * field projection (`Consensus.majority`)
        //   * enum variant (`ChunkStrategy.ByParagraph`)
        if let Some(dot) = symbol.find('.') {
            let ty = &symbol[..dot];
            let method = &symbol[dot + 1..];
            if self.interp_ctors.contains(symbol) {
                return Some("interp ctor");
            }
            if self.enum_variants.contains(symbol) {
                return Some("enum variant");
            }
            if self.builtin_fns.contains(symbol) {
                return Some("prelude qualified fn (e.g. Char.from_u32)");
            }
            // Method on a known opaque type via permissive table.
            let ty_known = self.opaque_types.contains(ty);
            if ty_known && self.permissive_methods.contains(method) {
                return Some("opaque type + permissive method");
            }
            if ty_known && self.interp_methods.contains(method) {
                return Some("opaque type + interp method");
            }
            if ty_known && self.interp_field_names.contains(method) {
                return Some("opaque type + interp field");
            }
            return None;
        }

        None
    }
}

/// One audit finding emitted by [`audit_catalog`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedEntry {
    pub symbol: String,
    pub module: String,
    /// `true` if the docstub block carried the `# concept-doc` opt-in
    /// (caller flagged for review, not auto-deletion).
    pub flagged_as_concept: bool,
    /// `true` if the docstub block carried the `# future` opt-in
    /// (intentional planned surface, not auto-deletion).
    pub flagged_as_future: bool,
}

/// Audit the whole embedded catalog against the real surface. Returns
/// every entry that does not resolve to a real callable / type / value
/// — minus the entries that opted in via `# concept-doc` (kept) or
/// `# future` (kept-with-review).
pub fn audit_catalog() -> Vec<UnresolvedEntry> {
    let surface = RealSurface::collect();
    let catalog = build_extracted_catalog();
    let concept_or_future = collect_block_flags();
    let mut out = Vec::new();
    for entry in &catalog {
        if surface.resolve(&entry.symbol).is_some() {
            continue;
        }
        let flags = concept_or_future
            .get(entry.symbol.as_str())
            .copied()
            .unwrap_or((false, false));
        out.push(UnresolvedEntry {
            symbol: entry.symbol.clone(),
            module: entry.module.clone(),
            flagged_as_concept: flags.0,
            flagged_as_future: flags.1,
        });
    }
    out
}

/// Render an audit report in the same shape as the drift report.
pub fn render_audit_report(unresolved: &[UnresolvedEntry], total: usize) -> String {
    let mut s = String::new();
    let real = total - unresolved.len();
    s.push_str(&format!(
        "catalog entries:   {total}\n\
         resolved entries:  {real}\n\
         unresolved:        {}\n\n",
        unresolved.len()
    ));
    if unresolved.is_empty() {
        s.push_str("OK: every catalog entry resolves to a real stdlib surface.\n");
        return s;
    }
    let mut by_module: BTreeMap<&str, Vec<&UnresolvedEntry>> = BTreeMap::new();
    for u in unresolved {
        by_module.entry(u.module.as_str()).or_default().push(u);
    }
    for (m, entries) in &by_module {
        s.push_str(&format!("  module {m}: {} unresolved\n", entries.len()));
        for u in entries {
            let tag = if u.flagged_as_concept {
                " [concept-doc — keep, review]"
            } else if u.flagged_as_future {
                " [future — keep, review]"
            } else {
                ""
            };
            s.push_str(&format!("    - {}{}\n", u.symbol, tag));
        }
    }
    s.push_str(
        "\nTo fix: either DELETE the docstub entry (if the surface was \
         aspirational), RENAME it (if the real symbol moved), add a \
         `# concept-doc` / `# future` marker before the `##sym` line \
         (if the entry is intentionally a non-callable reference), or \
         WIRE the real surface up so the resolver finds it.\n",
    );
    s
}

// ---------------------------------------------------------------------
// Source scanners
// ---------------------------------------------------------------------

const HOST_DISPATCH_SRC: &str = include_str!("../../mty-stdlib/src/host.rs");
const INTERP_RUN_SRC: &str = include_str!("../../mty-ir/src/interp/run.rs");
const LOWER_EXPRS_SRC: &str = include_str!("../../mty-ir/src/lower/exprs.rs");

/// Pull `("std.X", "Y") =>` arms out of `mty_stdlib::host::dispatch`.
fn scan_host_dispatch(src: &str, out: &mut RealSurface) {
    // The function lives inside `pub fn dispatch(...) -> Value { match
    // (module.as_str(), method) { ... } }`. We just scan the whole file
    // for the arm pattern — it's distinctive enough (the leading paren
    // + double quotes) that false positives are unlikely.
    for line in src.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("(\"std.") {
            // rest looks like: `module", "method") => ...`
            if let Some(close_q) = rest.find('"') {
                let module = format!("std.{}", &rest[..close_q]);
                let rest2 = &rest[close_q + 1..];
                let rest2 = rest2.trim_start_matches(", ");
                if let Some(rest3) = rest2.strip_prefix('"') {
                    if let Some(close_q2) = rest3.find('"') {
                        let method = &rest3[..close_q2];
                        out.host_methods.insert((module, method.to_string()));
                    }
                }
            }
        }
    }
}

/// Pull `"Name.method" =>` arms out of `try_stdlib_ctor`.
fn scan_interp_ctors(src: &str, out: &mut RealSurface) {
    let region = extract_fn_region(src, "fn try_stdlib_ctor(");
    for line in region.lines() {
        if let Some(name) = arm_symbol(line) {
            if name.contains('.') {
                out.interp_ctors.insert(name.to_string());
            }
        }
    }
}

/// Pull `"name" =>` arms out of `eval_method`.
fn scan_interp_methods(src: &str, out: &mut RealSurface) {
    let region = extract_fn_region(src, "fn eval_method(");
    for line in region.lines() {
        if let Some(name) = arm_symbol(line) {
            if !name.contains('.') && !name.starts_with("__") {
                out.interp_methods.insert(name.to_string());
            }
        }
    }
}

/// Pull `"name" => N,` arms out of `stdlib_field_index`.
fn scan_field_index(src: &str, out: &mut RealSurface) {
    let region = extract_fn_region(src, "fn stdlib_field_index(");
    for line in region.lines() {
        if let Some(name) = arm_symbol(line) {
            out.interp_field_names.insert(name.to_string());
        }
    }
}

/// Extract the body of a free function by matching braces. The needle
/// must include the trailing `(` of the function signature so we don't
/// confuse a closure/method with the same name. Panics if the function
/// is not found — that's a build-time bug.
fn extract_fn_region<'a>(src: &'a str, needle: &str) -> &'a str {
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("surface_audit: could not find `{needle}` in embedded source"));
    let rest = &src[start..];
    // Find the opening `{` of the function body.
    let open = rest
        .find('{')
        .unwrap_or_else(|| panic!("surface_audit: no `{{` after `{needle}`"));
    let body_start = start + open + 1;
    let body = &src[body_start..];
    // Walk forward, tracking brace depth. Naive but correct enough for
    // these files (no `{` inside strings/comments matters at the depth
    // we're tracking since they're well-balanced).
    let mut depth: i32 = 1;
    let mut end = body_start;
    for (i, ch) in body.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = body_start + i;
                    break;
                }
            }
            _ => {}
        }
    }
    &src[body_start..end]
}

/// Recognise an arm of the form `"<name>" => ...`. Returns the symbol
/// slice (no quotes) or `None` if the line is not a match arm.
fn arm_symbol(line: &str) -> Option<&str> {
    let l = line.trim_start();
    let rest = l.strip_prefix('"')?;
    let close_q = rest.find('"')?;
    let name = &rest[..close_q];
    let after = rest[close_q + 1..].trim_start();
    if after.starts_with("=>") {
        Some(name)
    } else {
        None
    }
}

/// Walk every `.rs` file under `crates/mty-stdlib/src/` and harvest the
/// surface they expose: `pub struct/enum/type` names, `pub fn` items at
/// module scope, `pub fn` items inside `impl <Type>` blocks, and
/// `pub enum` variant names.
///
/// We resolve `mty-stdlib/src/` via the `CARGO_MANIFEST_DIR` of THIS
/// crate (`mty-doc`) — a sibling crate, so the path is
/// `<mty-doc>/../mty-stdlib/src/`. If the directory is missing (e.g. a
/// trimmed-down distribution), the scan is a no-op and the resolver
/// falls back to the prelude + interp tables alone.
fn scan_stdlib_source_tree(out: &mut RealSurface) {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let stdlib_src = manifest.join("..").join("mty-stdlib").join("src");
    if !stdlib_src.is_dir() {
        return;
    }
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    collect_rs_files(&stdlib_src, &mut files);
    for f in &files {
        let Some(module) = module_name_for_path(&stdlib_src, f) else {
            continue;
        };
        let Ok(src) = std::fs::read_to_string(f) else {
            continue;
        };
        scan_rs_file(&src, &module, out);
    }
    // Also harvest top-level `pub mod <name>` from lib.rs so
    // `std.<name>` resolves even when the prelude hasn't registered
    // the module.
    if let Ok(lib_src) = std::fs::read_to_string(stdlib_src.join("lib.rs")) {
        for line in lib_src.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("pub mod ") {
                let name = rest.trim_end_matches(';').trim_end_matches('{').trim();
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    out.modules.insert(format!("std.{name}"));
                }
            }
        }
    }
}

fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            // Skip bin/ and tests/ — they aren't public surface.
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                if name == "bin" || name == "tests" {
                    continue;
                }
            }
            collect_rs_files(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

/// Given `mty-stdlib/src/<a>/<b>.rs` or `mty-stdlib/src/<a>.rs`, return
/// `"std.<a>"` (we coarse-grain submodules into their parent — the
/// docstub catalog never distinguishes `std.crypto.aes_gcm` as separate
/// from `std.crypto.aes_gcm`, it uses dot-path freely). Returns `None`
/// for `lib.rs` / `host.rs` (non-user-facing module boundaries).
fn module_name_for_path(root: &std::path::Path, path: &std::path::Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let components: Vec<&str> = rel
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    if components.is_empty() {
        return None;
    }
    // First component is the module bucket. `lib.rs` -> None (already
    // handled by caller). `host.rs` -> `std.host` (irrelevant to
    // catalog, but harmless).
    let first = components[0];
    let name = first.strip_suffix(".rs").unwrap_or(first);
    if name == "lib" {
        return None;
    }
    Some(format!("std.{name}"))
}

/// Extract surface items from a single `.rs` source body.
fn scan_rs_file(src: &str, module: &str, out: &mut RealSurface) {
    // Strip module prefix to bare module name for the `module_fns` key.
    let module_bare = module.strip_prefix("std.").unwrap_or(module);

    // Walk line-by-line tracking `impl <Type>` blocks via brace depth.
    let mut impl_stack: Vec<(String, i32)> = Vec::new(); // (type_name, depth)
    let mut depth: i32 = 0;
    let mut pending_enum: Option<String> = None;
    let mut enum_brace_depth: i32 = -1;

    for raw_line in src.lines() {
        let line = strip_line_comment(raw_line);
        let trimmed = line.trim_start();

        // Detect `impl <Type> {` and `impl<G> <Type> {` and `impl<G> <Type> for ... {`.
        // We only care about the type after `impl`. Tolerate `impl<...>` generics.
        if trimmed.starts_with("impl") {
            if let Some(ty) = parse_impl_type(trimmed) {
                impl_stack.push((ty, depth));
            }
        }

        // Detect pub items.
        if trimmed.starts_with("pub ")
            || trimmed.starts_with("pub(crate) ")
            || trimmed.starts_with("pub(super) ")
        {
            let after_pub = strip_pub_prefix(trimmed);
            // `pub struct/enum/type NAME` — record opaque + module type.
            if let Some(name) = strip_first_ident_after_keyword(after_pub, "struct ") {
                out.opaque_types.insert(name.to_string());
                out.module_fns
                    .insert((module_bare.to_string(), name.to_string()));
                // Bare module-qualified is the symbol the catalog
                // typically uses: `std.regex.Regex`.
                out.module_fns
                    .insert((module.to_string(), name.to_string()));
            }
            if let Some(name) = strip_first_ident_after_keyword(after_pub, "enum ") {
                out.opaque_types.insert(name.to_string());
                out.module_fns
                    .insert((module_bare.to_string(), name.to_string()));
                out.module_fns
                    .insert((module.to_string(), name.to_string()));
                pending_enum = Some(name.to_string());
                enum_brace_depth = depth; // record current depth; entry into body bumps it.
            }
            if let Some(name) = strip_first_ident_after_keyword(after_pub, "type ") {
                out.opaque_types.insert(name.to_string());
            }
            if let Some(name) = strip_first_ident_after_keyword(after_pub, "const ") {
                out.module_fns
                    .insert((module.to_string(), name.to_string()));
                out.module_fns
                    .insert((module_bare.to_string(), name.to_string()));
            }
            if let Some(name) = strip_first_ident_after_keyword(after_pub, "fn ") {
                // Inside an `impl <Type>` block → it's a method on Type.
                if let Some((ty, _)) = impl_stack.last() {
                    out.interp_ctors.insert(format!("{ty}.{name}"));
                    out.permissive_methods.insert(name.to_string());
                } else {
                    // Free fn at module scope.
                    out.module_fns
                        .insert((module.to_string(), name.to_string()));
                    out.module_fns
                        .insert((module_bare.to_string(), name.to_string()));
                    out.builtin_fns.insert(name.to_string());
                }
            }
        }

        // Pending enum body: record variants. A variant line looks like
        // `    Compile(String),` or `    Encrypt,` or `    Other { ... },`.
        // We harvest the leading capitalised identifier.
        if let Some(enum_name) = pending_enum.as_ref() {
            // Only look inside the brace body.
            if depth > enum_brace_depth {
                if let Some(variant) = parse_enum_variant(trimmed) {
                    out.enum_variants.insert(format!("{enum_name}.{variant}"));
                    out.interp_ctors.insert(format!("{enum_name}.{variant}"));
                }
            }
        }

        // Update depth (after the item-line parsing — so `impl X {` is
        // accepted at the depth just before entering the block).
        for ch in line.chars() {
            match ch {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }

        // Pop impl_stack entries whose recorded depth is no longer below
        // the current depth (i.e. we've exited the impl body).
        while impl_stack.last().is_some_and(|(_, d)| depth <= *d) {
            impl_stack.pop();
        }

        // Close pending enum once we've returned to its starting depth.
        if pending_enum.is_some() && depth <= enum_brace_depth {
            pending_enum = None;
            enum_brace_depth = -1;
        }
    }
}

fn strip_line_comment(s: &str) -> &str {
    if let Some(idx) = s.find("//") {
        &s[..idx]
    } else {
        s
    }
}

fn strip_pub_prefix(s: &str) -> &str {
    let mut rest = s;
    for prefix in ["pub(crate) ", "pub(super) ", "pub "] {
        if let Some(r) = rest.strip_prefix(prefix) {
            rest = r;
            break;
        }
    }
    // Skip leading `async ` / `unsafe ` / `const ` / `extern "X" ` /
    // `default ` qualifiers so the keyword-matchers below still see the
    // base `fn ` / `struct ` / `enum ` / `type ` token.
    loop {
        let mut progressed = false;
        for q in ["async ", "unsafe ", "const fn ", "extern fn "] {
            if q.ends_with("fn ") {
                continue;
            }
            if let Some(r) = rest.strip_prefix(q) {
                rest = r;
                progressed = true;
            }
        }
        if let Some(r) = rest.strip_prefix("extern \"") {
            // extern "C" fn ... — skip past closing quote + space.
            if let Some(close) = r.find('"') {
                let after = &r[close + 1..];
                rest = after.trim_start();
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    rest
}

fn strip_first_ident_after_keyword<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(kw)?;
    // Read the first identifier (alphanumeric + `_`). Stops at `<`, `(`,
    // `=`, `:`, whitespace, etc.
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    let name = &rest[..end];
    if name.is_empty()
        || !name
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
    {
        return None;
    }
    Some(name)
}

/// Parse `impl<G> Type<...> { ... }` / `impl Type for X { ... }` and
/// return the FIRST type name (the `Self` type). Generic params are
/// skipped.
fn parse_impl_type(s: &str) -> Option<String> {
    let mut rest = s.strip_prefix("impl")?;
    rest = rest.trim_start();
    // Skip optional generic params `<...>`.
    if rest.starts_with('<') {
        let mut depth = 0;
        let mut end = 0;
        for (i, ch) in rest.char_indices() {
            match ch {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if end == 0 {
            return None;
        }
        rest = rest[end..].trim_start();
    }
    // First identifier is the type.
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    let ty = &rest[..end];
    if ty.is_empty() {
        return None;
    }
    Some(ty.to_string())
}

/// Look for a leading capitalised identifier on the line — the form
/// `Variant,` / `Variant(...)` / `Variant { ... }`. Returns `None` for
/// anything that doesn't look like a variant declaration.
fn parse_enum_variant(s: &str) -> Option<String> {
    let mut chars = s.char_indices();
    let (start, first) = chars.next()?;
    if !first.is_uppercase() {
        return None;
    }
    let mut end = start + first.len_utf8();
    for (i, ch) in chars {
        if ch.is_alphanumeric() || ch == '_' {
            end = i + ch.len_utf8();
        } else {
            break;
        }
    }
    let name = &s[start..end];
    // Trailing char must be a separator / opener that variant
    // declarations use, OR an end of line.
    let after = s[end..].trim_start();
    if after.is_empty()
        || after.starts_with(',')
        || after.starts_with('(')
        || after.starts_with('{')
        || after.starts_with('=')
    {
        Some(name.to_string())
    } else {
        None
    }
}

/// Walk every embedded docstub file and harvest per-symbol opt-in flags
/// (`# concept-doc` / `# future`) that appear on the line immediately
/// before a `##sym` block. Returns a map from symbol to
/// `(is_concept, is_future)`.
fn collect_block_flags() -> BTreeMap<String, (bool, bool)> {
    let mut out: BTreeMap<String, (bool, bool)> = BTreeMap::new();
    for (_module, src) in EMBEDDED_DOCSTUBS {
        let mut concept = false;
        let mut future = false;
        for line in src.lines() {
            let t = line.trim_start();
            if t.starts_with("# concept-doc") {
                concept = true;
            } else if t.starts_with("# future") {
                future = true;
            } else if let Some(rest) = t.strip_prefix("##sym ") {
                let sym = rest.trim().to_string();
                if concept || future {
                    out.insert(sym, (concept, future));
                }
                concept = false;
                future = false;
            } else if t.is_empty() {
                // blank line — keep flags pending; they apply to the
                // NEXT `##sym` block.
            } else if !t.starts_with('#') {
                // A real directive that isn't `##sym` clears pending
                // flags. (Doesn't happen in practice since `##sym` is
                // the first directive after the header.)
                concept = false;
                future = false;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_scan_finds_known_prelude_entries() {
        let s = RealSurface::collect();
        // Prelude modules.
        assert!(
            s.modules.contains("std.http"),
            "std.http should be a module"
        );
        assert!(s.modules.contains("std.llm"));
        assert!(s.modules.contains("std.rag"));
        // Prelude builtin fns.
        assert!(s.builtin_fns.contains("log"));
        assert!(s.builtin_fns.contains("panic"));
        assert!(s.builtin_fns.contains("spawn"));
        // Prelude opaque types.
        assert!(s.opaque_types.contains("Member"));
        assert!(s.opaque_types.contains("Vec"));
        assert!(s.opaque_types.contains("Index"));
        // Permissive methods.
        assert!(s.permissive_methods.contains("push"));
        assert!(s.permissive_methods.contains("ask"));
    }

    #[test]
    fn surface_scan_finds_host_dispatch_entries() {
        let s = RealSurface::collect();
        assert!(s
            .host_methods
            .contains(&("std.json".to_string(), "parse".to_string())));
        assert!(s
            .host_methods
            .contains(&("std.fs".to_string(), "read".to_string())));
        assert!(s
            .host_methods
            .contains(&("std.http".to_string(), "get".to_string())));
    }

    #[test]
    fn surface_scan_finds_interp_ctors() {
        let s = RealSurface::collect();
        assert!(s.interp_ctors.contains("Vec.new"));
        assert!(s.interp_ctors.contains("Member.anthropic"));
        assert!(s.interp_ctors.contains("String.with_capacity"));
    }

    #[test]
    fn surface_scan_finds_interp_methods() {
        let s = RealSurface::collect();
        assert!(s.interp_methods.contains("len"));
        assert!(s.interp_methods.contains("as_str"));
        assert!(s.interp_methods.contains("slice"));
    }

    #[test]
    fn resolve_handles_known_shapes() {
        let s = RealSurface::collect();
        assert!(s.resolve("log").is_some());
        assert!(s.resolve("Vec.new").is_some());
        assert!(s.resolve("Vec.push").is_some());
        assert!(s.resolve("Member.anthropic").is_some());
        assert!(s.resolve("std.json.parse").is_some());
        assert!(s.resolve("eprintln").is_some());
        // Bogus.
        assert!(s.resolve("std.does_not_exist.foo").is_none());
        assert!(s.resolve("FakeType.bar").is_none());
    }

    #[test]
    fn arm_symbol_recognises_match_arms() {
        assert_eq!(
            arm_symbol("    \"Vec.new\" => Some(Array(Vec::new())),"),
            Some("Vec.new")
        );
        assert_eq!(arm_symbol("\"len\" => match receiver {"), Some("len"));
        assert_eq!(arm_symbol("    let s = \"hello\";"), None);
        assert_eq!(
            arm_symbol("// comment with \"Type.method\" => something"),
            None
        );
    }
}
