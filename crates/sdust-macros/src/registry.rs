//! The compile-time registry of declarative macros visible in a
//! translation unit.
//!
//! The registry is built once during HIR lowering by walking the CST
//! and collecting every top-level `MACRO_DECL`. Macros are looked up by
//! their declared name; v0.4 does not support overloading or scoped
//! macros, so the registry is a flat `HashMap`.

use crate::token::{tokens_from_body_node, Tok};
use sdust_ast::AstNode;
use sdust_syntax::{SyntaxKind, SyntaxNode};
use std::collections::HashMap;

/// A declarative macro: name, ordered parameter list, and the opaque
/// token body extracted from the CST.
#[derive(Debug, Clone)]
pub struct MacroDef {
    pub name: String,
    pub params: Vec<String>,
    /// The body's leaf tokens, excluding the outer braces. Trivia is
    /// preserved so expanded source stays readable.
    pub body: Vec<Tok>,
}

impl MacroDef {
    pub fn arity(&self) -> usize {
        self.params.len()
    }

    /// Returns true if `ident` is one of the macro's parameter names.
    pub fn is_param(&self, ident: &str) -> bool {
        self.params.iter().any(|p| p == ident)
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

    /// Walk a CST File node and ingest every top-level `MACRO_DECL`.
    /// Malformed macro decls (missing name, unbalanced braces) are
    /// silently skipped — the parser already raised diagnostics for
    /// those.
    pub fn from_file(file: &SyntaxNode) -> Self {
        let mut reg = Self::new();
        for child in file.children() {
            if child.kind() == SyntaxKind::MACRO_DECL {
                if let Some(def) = lower_macro_decl(&child) {
                    reg.insert(def);
                }
            }
        }
        reg
    }
}

/// Extract a [`MacroDef`] from a `MACRO_DECL` CST node.
pub fn lower_macro_decl(node: &SyntaxNode) -> Option<MacroDef> {
    debug_assert_eq!(node.kind(), SyntaxKind::MACRO_DECL);

    // First NAME child is the macro's own name; remaining NAME children
    // (until the body) are parameter names.
    let mut names = node.children().filter_map(sdust_ast::Name::cast);
    let name = names.next()?.text();
    let params: Vec<String> = names.map(|n| n.text()).collect();

    // The body is the brace-balanced token run that follows `=>`. The
    // macro_decl parser doesn't wrap it in a node, so we recover by
    // collecting every leaf token after the first `{`, stopping when
    // the matching `}` is reached. Outer braces are excluded.
    let body = extract_body_tokens(node);

    Some(MacroDef { name, params, body })
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
    use sdust_ast::{AstNode, File};

    fn parse(src: &str) -> SyntaxNode {
        let p = sdust_syntax::parse(src);
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
}
