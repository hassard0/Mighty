//! Typed AST view over the rowan CST.
pub use mty_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

pub trait AstNode: Sized {
    fn can_cast(kind: SyntaxKind) -> bool;
    fn cast(node: SyntaxNode) -> Option<Self>;
    fn syntax(&self) -> &SyntaxNode;
}

macro_rules! ast_node {
    ($name:ident, $kind:ident) => {
        #[derive(Debug, Clone)]
        pub struct $name(pub SyntaxNode);
        impl AstNode for $name {
            fn can_cast(kind: SyntaxKind) -> bool {
                kind == SyntaxKind::$kind
            }
            fn cast(node: SyntaxNode) -> Option<Self> {
                if Self::can_cast(node.kind()) {
                    Some(Self(node))
                } else {
                    None
                }
            }
            fn syntax(&self) -> &SyntaxNode {
                &self.0
            }
        }
    };
}

#[allow(unused_imports)]
pub(crate) use ast_node;

mod generated;
pub use generated::*;

mod effects;
pub use effects::*;

// v0.27 Track A: typed accessors for `@tool(...)` attribute prefix.
// The module is brought into scope so its `impl ToolAttr` blocks
// register on the generated ToolAttr struct; no symbols re-exported
// because the generated module already exports the types.
mod items;
