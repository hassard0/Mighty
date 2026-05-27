//! v0.27 Track A — typed AST accessors for the `@tool(...)` attribute
//! prefix.
//!
//! The parser emits the attribute as a sibling node of the FN_DECL
//! (under the same item checkpoint). HIR lowering / the macro
//! preprocessor walk a fn-with-tool-attr by:
//!
//!   1. Finding a `FN_DECL` child of the file.
//!   2. Calling [`ToolAttr::for_fn_decl`] to locate the attribute that
//!      immediately precedes it (sibling lookup — the attribute is
//!      always emitted before the fn).
//!   3. Reading the description string + (optional) cap expression text
//!      via [`ToolAttr::description_literal`] / [`ToolAttr::cap_expr_text`].
//!
//! No new node kinds are introduced here — everything routes through
//! the rowan tree the parser already produced.

use crate::generated::{ToolAttr, ToolAttrArgs, ToolAttrCapArg};
use crate::AstNode;
use mty_syntax::{SyntaxKind, SyntaxNode};

impl ToolAttr {
    /// Find the [`ToolAttr`] attached to `fn_decl`. Returns `None`
    /// when the fn carries no `@tool` attribute.
    ///
    /// The parser opens the FN_DECL checkpoint BEFORE consuming the
    /// `@<name>(args)` prefix (mirroring how `#[derive(...)]` is
    /// captured under the same item via the slice-5 checkpoint
    /// pattern). The result: `TOOL_ATTR` is a CHILD of `FN_DECL`, not
    /// a preceding sibling. This accessor takes the first
    /// `TOOL_ATTR` child it finds — v0.27 emits at most one.
    pub fn for_fn_decl(fn_decl: &SyntaxNode) -> Option<Self> {
        debug_assert_eq!(fn_decl.kind(), SyntaxKind::FN_DECL);
        fn_decl.children().find_map(Self::cast)
    }

    /// The attribute name (e.g. `"tool"`). v0.27 accepts only `tool`,
    /// but the accessor stays generic for the v0.28 surface.
    pub fn name(&self) -> String {
        self.0
            .children()
            .find(|c| c.kind() == SyntaxKind::NAME)
            .and_then(|n| n.first_token())
            .map(|t| t.text().to_string())
            .unwrap_or_default()
    }

    /// The `(...)` argument list node. Always present when the parser
    /// produced a TOOL_ATTR (the parser requires a paren on the prefix
    /// check).
    pub fn args(&self) -> Option<ToolAttrArgs> {
        self.0.children().find_map(ToolAttrArgs::cast)
    }

    /// The first positional arg's raw source text (description literal).
    /// Returns `None` when the attribute has no positional args.
    pub fn description_literal(&self) -> Option<String> {
        let args = self.args()?;
        args.0
            .children()
            .find(|c| c.kind() == SyntaxKind::ARG)
            .map(|n| n.text().to_string().trim().to_string())
    }

    /// The `cap:` expression's raw source text. Returns `None` when
    /// the attribute has no `cap:` argument.
    pub fn cap_expr_text(&self) -> Option<String> {
        let args = self.args()?;
        args.0.children().find_map(ToolAttrCapArg::cast).map(|c| {
            // Skip the `cap` NAME + `:` punctuation; everything after
            // is the expression text.
            let mut found_colon = false;
            let mut out = String::new();
            for elem in c.0.children_with_tokens() {
                if !found_colon {
                    if let Some(tok) = elem.as_token() {
                        if tok.kind() == SyntaxKind::COLON {
                            found_colon = true;
                        }
                    }
                    continue;
                }
                match elem {
                    rowan::NodeOrToken::Node(n) => out.push_str(&n.text().to_string()),
                    rowan::NodeOrToken::Token(t) => {
                        if !t.kind().is_trivia() {
                            out.push_str(t.text());
                        } else if !out.is_empty() {
                            // preserve internal whitespace for round-trips
                            out.push_str(t.text());
                        }
                    }
                }
            }
            out.trim().to_string()
        })
    }

    /// All other named args (`streaming: true`, `name: "x"`, etc.) as
    /// `(name, raw_value_text)` pairs. Excludes the special-cased
    /// `cap:` arg (use [`Self::cap_expr_text`]).
    pub fn named_args(&self) -> Vec<(String, String)> {
        let Some(args) = self.args() else {
            return vec![];
        };
        args.0
            .children()
            .filter(|c| c.kind() == SyntaxKind::NAMED_ARG)
            .filter_map(|n| {
                let name = n
                    .children()
                    .find(|c| c.kind() == SyntaxKind::NAME)
                    .and_then(|x| x.first_token())
                    .map(|t| t.text().to_string())?;
                // Value: everything after the COLON token.
                let mut found_colon = false;
                let mut out = String::new();
                for elem in n.children_with_tokens() {
                    if !found_colon {
                        if let Some(tok) = elem.as_token() {
                            if tok.kind() == SyntaxKind::COLON {
                                found_colon = true;
                            }
                        }
                        continue;
                    }
                    match elem {
                        rowan::NodeOrToken::Node(nn) => out.push_str(&nn.text().to_string()),
                        rowan::NodeOrToken::Token(t) => {
                            if !t.kind().is_trivia() || !out.is_empty() {
                                out.push_str(t.text());
                            }
                        }
                    }
                }
                Some((name, out.trim().to_string()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::{File, FnDecl};

    fn first_fn(src: &str) -> SyntaxNode {
        let r = mty_syntax::parse(src);
        let root = SyntaxNode::new_root(r.green);
        let file = File::cast(root).expect("file");
        file.0
            .children()
            .find(|c| c.kind() == SyntaxKind::FN_DECL)
            .expect("fn decl")
    }

    #[test]
    fn finds_tool_attr_for_fn() {
        let fn_decl = first_fn("@tool(\"desc\") fn foo() {}");
        let attr = ToolAttr::for_fn_decl(&fn_decl).expect("attr present");
        assert_eq!(attr.name(), "tool");
    }

    #[test]
    fn description_literal_round_trips() {
        let fn_decl = first_fn("@tool(\"hello\") fn foo() {}");
        let attr = ToolAttr::for_fn_decl(&fn_decl).expect("attr present");
        // Includes the surrounding quotes — the macro expander
        // (mty_macros::stdlib::tool::decode_string_literal) strips them.
        assert_eq!(attr.description_literal().as_deref(), Some("\"hello\""));
    }

    #[test]
    fn cap_expr_text_captures_dotted_path() {
        let fn_decl = first_fn("@tool(\"d\", cap: fs.read) fn foo() {}");
        let attr = ToolAttr::for_fn_decl(&fn_decl).expect("attr present");
        assert_eq!(attr.cap_expr_text().as_deref(), Some("fs.read"));
    }

    #[test]
    fn cap_expr_text_captures_method_call() {
        let fn_decl = first_fn("@tool(\"d\", cap: fs.read(\"./data/**\")) fn foo() {}");
        let attr = ToolAttr::for_fn_decl(&fn_decl).expect("attr present");
        let cap = attr.cap_expr_text().expect("cap present");
        // Whitespace inside the literal isn't normalized — just make
        // sure both the method name and the path argument survived.
        assert!(cap.contains("fs.read"), "got: {}", cap);
        assert!(cap.contains("./data/**"), "got: {}", cap);
    }

    #[test]
    fn named_args_collected() {
        let fn_decl =
            first_fn("@tool(\"d\", cap: fs.read, streaming: true, name: \"rd\") fn foo() {}");
        let attr = ToolAttr::for_fn_decl(&fn_decl).expect("attr present");
        let na = attr.named_args();
        let streaming = na.iter().find(|(n, _)| n == "streaming");
        let name = na.iter().find(|(n, _)| n == "name");
        assert!(streaming.is_some(), "streaming arg missing: {:?}", na);
        assert!(name.is_some(), "name arg missing: {:?}", na);
        // `cap:` is special-cased — not in named_args.
        assert!(na.iter().all(|(n, _)| n != "cap"));
    }

    #[test]
    fn fn_without_attr_returns_none() {
        let fn_decl = first_fn("fn plain() {}");
        assert!(ToolAttr::for_fn_decl(&fn_decl).is_none());
    }

    #[test]
    fn fn_decl_typed_accessor_compiles() {
        // Smoke: confirm the FnDecl typed accessor still works on
        // attribute-prefixed source (no regression to the v0.5 derive
        // shape).
        let fn_decl = first_fn("@tool(\"d\") fn read_doc() {}");
        let fd = FnDecl::cast(fn_decl).expect("FnDecl");
        assert_eq!(fd.name().map(|n| n.text()).as_deref(), Some("read_doc"));
    }
}
