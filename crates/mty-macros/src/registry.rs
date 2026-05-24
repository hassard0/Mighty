//! The compile-time registry of declarative macros visible in a
//! translation unit.
//!
//! v0.5 changes from v0.4:
//!
//!   * `MacroDef` gains a `MacroKind` (declarative vs procedural) and an
//!     `is_pub` flag. Procedural macros store the body but do not yet
//!     execute (gated by MT6006 — see `proc` module).
//!   * A new [`PackageMacros`] type splits a package's macros into
//!     `local` (visible only inside the file/package) and `exported`
//!     (re-exportable via `pub macro`). Cross-file resolution copies
//!     exported defs into the importing file's local registry via
//!     [`PackageMacros::register_use`].
//!
//! The registry is built once during HIR lowering by walking the CST
//! and collecting every top-level `MACRO_DECL` / `PROC_MACRO_DECL`.
//! Macros are looked up by their declared name; v0.5 still does not
//! support overloading or scoped macros, so each map is a flat
//! `HashMap`.

use crate::token::{tokens_from_body_node, Tok};
use mty_ast::AstNode;
use mty_syntax::{SyntaxKind, SyntaxNode};
use std::collections::HashMap;

/// What flavor of macro this is. Declarative macros are token-tree
/// rewriters (the v0.4 surface); procedural macros are Mighty
/// functions over `TokenStream` (v0.5 parses + stores, v0.6 will run).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroKind {
    /// Declarative: parameter substitution + hygiene mangling.
    Declarative,
    /// Procedural: body is a fn-shape that maps `TokenStream` →
    /// `TokenStream`. v0.5 stores the body but emits MT6006 at call
    /// sites (deferred to v0.6).
    Procedural,
}

/// A declarative or procedural macro: name, ordered parameter list,
/// the opaque token body extracted from the CST, plus v0.5 metadata.
#[derive(Debug, Clone)]
pub struct MacroDef {
    pub name: String,
    pub params: Vec<String>,
    /// The body's leaf tokens, excluding the outer braces. Trivia is
    /// preserved so expanded source stays readable.
    pub body: Vec<Tok>,
    /// True if the source declared `pub macro …` — eligible for
    /// cross-file import via [`PackageMacros::register_use`].
    pub is_pub: bool,
    /// Declarative vs procedural.
    pub kind: MacroKind,
}

impl MacroDef {
    pub fn arity(&self) -> usize {
        self.params.len()
    }

    /// Returns true if `ident` is one of the macro's parameter names.
    pub fn is_param(&self, ident: &str) -> bool {
        self.params.iter().any(|p| p == ident)
    }

    /// True for declarative macros that the v0.5 expander can run.
    pub fn is_declarative(&self) -> bool {
        matches!(self.kind, MacroKind::Declarative)
    }

    /// True for procedural macros (parsed-only in v0.5).
    pub fn is_procedural(&self) -> bool {
        matches!(self.kind, MacroKind::Procedural)
    }
}

/// Lookup table for all macros visible in a translation unit.
#[derive(Debug, Default, Clone)]
pub struct MacroRegistry {
    pub macros: HashMap<String, MacroDef>,
}

impl MacroRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, def: MacroDef) {
        self.macros.insert(def.name.clone(), def);
    }

    pub fn get(&self, name: &str) -> Option<&MacroDef> {
        self.macros.get(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.macros.contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.macros.len()
    }

    pub fn is_empty(&self) -> bool {
        self.macros.is_empty()
    }

    /// Walk a CST File node and ingest every top-level `MACRO_DECL` and
    /// `PROC_MACRO_DECL`. Malformed decls (missing name, unbalanced
    /// braces) are silently skipped — the parser already raised
    /// diagnostics for those.
    pub fn from_file(file: &SyntaxNode) -> Self {
        let mut reg = Self::new();
        for child in file.children() {
            match child.kind() {
                SyntaxKind::MACRO_DECL => {
                    if let Some(def) = lower_macro_decl(&child) {
                        reg.insert(def);
                    }
                }
                SyntaxKind::PROC_MACRO_DECL => {
                    if let Some(def) = lower_proc_macro_decl(&child) {
                        reg.insert(def);
                    }
                }
                _ => {}
            }
        }
        reg
    }
}

/// Per-package macro registry: splits visibility into local and exported.
///
/// `local` is what the file's expander sees (its own decls + anything
/// imported via `use`). `exported` is the subset of `local` whose
/// declarations carried `pub macro` (or `pub proc macro`).
#[derive(Debug, Default, Clone)]
pub struct PackageMacros {
    pub local: MacroRegistry,
    pub exported: MacroRegistry,
}

impl PackageMacros {
    pub fn new() -> Self {
        Self::default()
    }

    /// Walk a parsed file, populating both maps. Every macro lands in
    /// `local`; those whose source carried `pub` also land in
    /// `exported`.
    pub fn from_file(file: &SyntaxNode) -> Self {
        let mut pm = Self::new();
        for child in file.children() {
            let def_opt = match child.kind() {
                SyntaxKind::MACRO_DECL => lower_macro_decl(&child),
                SyntaxKind::PROC_MACRO_DECL => lower_proc_macro_decl(&child),
                _ => None,
            };
            if let Some(def) = def_opt {
                if def.is_pub {
                    pm.exported.insert(def.clone());
                }
                pm.local.insert(def);
            }
        }
        pm
    }

    /// Pull every macro out of `other`'s `exported` set and merge into
    /// `self.local`. Used when the importer's `use otherpkg.foo`
    /// resolves a macro symbol. Optionally rename via `alias_map`
    /// (e.g. `use otherpkg.foo as bar` → entry `("foo", "bar")`).
    pub fn register_use(&mut self, other: &PackageMacros, alias_map: &[(String, String)]) {
        for (name, def) in &other.exported.macros {
            let bound_name = alias_map
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| name.clone());
            let mut clone = def.clone();
            clone.name = bound_name.clone();
            self.local.macros.insert(bound_name, clone);
        }
    }

    /// Pull a single named macro out of `other`'s `exported` set into
    /// `self.local`. Returns true if the symbol existed.
    pub fn register_use_one(&mut self, other: &PackageMacros, name: &str, bound_as: &str) -> bool {
        if let Some(def) = other.exported.macros.get(name) {
            let mut clone = def.clone();
            clone.name = bound_as.to_string();
            self.local.macros.insert(bound_as.to_string(), clone);
            true
        } else {
            false
        }
    }
}

/// Extract a [`MacroDef`] from a `MACRO_DECL` CST node.
pub fn lower_macro_decl(node: &SyntaxNode) -> Option<MacroDef> {
    debug_assert_eq!(node.kind(), SyntaxKind::MACRO_DECL);

    // First NAME child is the macro's own name; remaining NAME children
    // (until the body) are parameter names.
    let mut names = node.children().filter_map(mty_ast::Name::cast);
    let name = names.next()?.text();
    let params: Vec<String> = names.map(|n| n.text()).collect();

    let is_pub = decl_is_pub(node);

    // The body is the brace-balanced token run that follows `=>`. The
    // macro_decl parser doesn't wrap it in a node, so we recover by
    // collecting every leaf token after the first `{`, stopping when
    // the matching `}` is reached. Outer braces are excluded.
    let body = extract_body_tokens(node);

    Some(MacroDef {
        name,
        params,
        body,
        is_pub,
        kind: MacroKind::Declarative,
    })
}

/// Extract a [`MacroDef`] from a `PROC_MACRO_DECL` CST node.
pub fn lower_proc_macro_decl(node: &SyntaxNode) -> Option<MacroDef> {
    debug_assert_eq!(node.kind(), SyntaxKind::PROC_MACRO_DECL);

    let mut names = node.children().filter_map(mty_ast::Name::cast);
    let name = names.next()?.text();
    // The proc-macro parser stores the input param's IDENT under NAME
    // (e.g. `input`). Subsequent NAMEs are unlikely but we collect them
    // for parity with declarative macros so error messages can report
    // the param name.
    let params: Vec<String> = names.map(|n| n.text()).collect();

    let is_pub = decl_is_pub(node);
    let body = extract_body_tokens(node);

    Some(MacroDef {
        name,
        params,
        body,
        is_pub,
        kind: MacroKind::Procedural,
    })
}

/// True if the decl is preceded by a `VISIBILITY` sibling whose first
/// token is `pub`. The visibility node is parsed *before* the decl
/// keyword and sits under the same parent (FILE).
fn decl_is_pub(node: &SyntaxNode) -> bool {
    // Walk previous siblings until we find a non-trivia node. If it's a
    // VISIBILITY containing `pub`, we're public.
    let mut sib = node.prev_sibling();
    while let Some(s) = sib {
        if s.kind() == SyntaxKind::VISIBILITY {
            return s.first_token().map(|t| t.text() == "pub").unwrap_or(false);
        }
        sib = s.prev_sibling();
    }
    // Visibility may also appear as the first token *of* the decl
    // (the `pub` is bumped under the decl's own checkpoint). Check
    // the first non-trivia token of the decl itself.
    for tok in node.descendants_with_tokens() {
        let Some(t) = tok.into_token() else { continue };
        if t.kind().is_trivia() {
            continue;
        }
        return t.text() == "pub";
    }
    false
}

fn extract_body_tokens(node: &SyntaxNode) -> Vec<Tok> {
    // Strategy: walk the node's direct children-with-tokens, find the
    // first L_BRACE, then accumulate tokens until the matching R_BRACE.
    // The macro_decl parser stores body tokens flat under MACRO_DECL,
    // so descendants_with_tokens captures them in order.
    let mut out = vec![];
    let mut depth = 0i32;
    let mut started = false;
    for elem in node.descendants_with_tokens() {
        let Some(t) = elem.into_token() else { continue };
        match t.kind() {
            SyntaxKind::L_BRACE => {
                if !started {
                    started = true;
                    depth = 1;
                    continue; // skip outer `{`
                }
                depth += 1;
                out.push(Tok::new(t.kind(), t.text().to_string()));
            }
            SyntaxKind::R_BRACE => {
                depth -= 1;
                if depth == 0 {
                    break; // skip outer `}`
                }
                out.push(Tok::new(t.kind(), t.text().to_string()));
            }
            _ => {
                if started {
                    out.push(Tok::new(t.kind(), t.text().to_string()));
                }
            }
        }
    }
    // Trim leading/trailing whitespace so the body is tightly bounded.
    while out.first().map(|t| t.is_trivia()).unwrap_or(false) {
        out.remove(0);
    }
    while out.last().map(|t| t.is_trivia()).unwrap_or(false) {
        out.pop();
    }
    let _ = tokens_from_body_node; // keep the helper exported even if unused here
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mty_ast::{AstNode, File};

    fn parse(src: &str) -> SyntaxNode {
        let p = mty_syntax::parse(src);
        let root = SyntaxNode::new_root(p.green);
        File::cast(root).unwrap().0
    }

    #[test]
    fn registers_a_single_macro() {
        let file = parse("macro foo() => { 42 }\n");
        let reg = MacroRegistry::from_file(&file);
        assert_eq!(reg.len(), 1);
        let def = reg.get("foo").unwrap();
        assert_eq!(def.name, "foo");
        assert!(def.params.is_empty());
        // body should contain at least the int literal `42`
        assert!(def
            .body
            .iter()
            .any(|t| t.kind == SyntaxKind::INT_LITERAL && t.text == "42"));
        assert!(!def.is_pub);
        assert!(def.is_declarative());
    }

    #[test]
    fn registers_macro_with_params() {
        let file = parse("macro id(x) => { x }\n");
        let reg = MacroRegistry::from_file(&file);
        let def = reg.get("id").unwrap();
        assert_eq!(def.params, vec!["x"]);
    }

    #[test]
    fn registers_assert_eq() {
        let src = "macro assert_eq(a, b) => { if a != b { panic(\"oops\") } }\n";
        let file = parse(src);
        let reg = MacroRegistry::from_file(&file);
        let def = reg.get("assert_eq").unwrap();
        assert_eq!(def.params, vec!["a", "b"]);
        // The body should reference both params and `panic`.
        let texts: Vec<&str> = def.body.iter().map(|t| t.text.as_str()).collect();
        assert!(texts.contains(&"a"));
        assert!(texts.contains(&"b"));
        assert!(texts.contains(&"panic"));
        assert!(texts.contains(&"!="));
    }

    #[test]
    fn detects_pub_macro() {
        let file = parse("pub macro greet() => { print(\"hi\") }\n");
        let pm = PackageMacros::from_file(&file);
        assert!(pm.local.contains("greet"));
        assert!(pm.exported.contains("greet"));
        assert!(pm.local.get("greet").unwrap().is_pub);
    }

    #[test]
    fn private_macros_stay_local_only() {
        let file = parse("macro priv() => { 1 }\npub macro pubm() => { 2 }\n");
        let pm = PackageMacros::from_file(&file);
        assert!(pm.local.contains("priv"));
        assert!(pm.local.contains("pubm"));
        assert!(!pm.exported.contains("priv"));
        assert!(pm.exported.contains("pubm"));
    }

    #[test]
    fn register_use_pulls_exported_macros() {
        let exporter_src = "pub macro greet() => { print(\"hi\") }\n";
        let exporter = PackageMacros::from_file(&parse(exporter_src));
        let mut importer = PackageMacros::new();
        importer.register_use(&exporter, &[]);
        assert!(importer.local.contains("greet"));
    }

    #[test]
    fn register_use_one_with_alias() {
        let exporter_src = "pub macro greet() => { print(\"hi\") }\n";
        let exporter = PackageMacros::from_file(&parse(exporter_src));
        let mut importer = PackageMacros::new();
        assert!(importer.register_use_one(&exporter, "greet", "hello"));
        assert!(importer.local.contains("hello"));
        assert!(!importer.local.contains("greet"));
    }

    #[test]
    fn proc_macro_decl_registers_as_procedural() {
        let src = "proc macro upcase(input: TokenStream) -> TokenStream { input }\n";
        let file = parse(src);
        let reg = MacroRegistry::from_file(&file);
        let def = reg.get("upcase").expect("proc macro registered");
        assert!(def.is_procedural());
        assert!(!def.is_declarative());
    }
}
