//! HIR lowering — populated by Tasks 21, 22.

use crate::ids::*;
use crate::nodes::*;
use mty_diagnostics::Diagnostic;

pub mod agents;
pub mod exprs;
pub mod items;
pub mod macros;
pub mod patterns;
pub mod types;

pub struct LoweringCtx {
    pub package: Package,
    pub diagnostics: Vec<Diagnostic>,
}

impl Default for LoweringCtx {
    fn default() -> Self {
        Self::new()
    }
}

impl LoweringCtx {
    pub fn new() -> Self {
        Self {
            package: Package::default(),
            diagnostics: vec![],
        }
    }

    pub fn lower_file(mut self, file: mty_ast::File) -> (Package, Vec<Diagnostic>) {
        // v0.4: pre-expand declarative macros (see mty_macros). If the
        // file has macro decls AND call sites, we rewrite the source,
        // re-parse, and proceed with the expanded CST. The original
        // diagnostics from the expander are surfaced through `self`.
        let original_src = file.0.text().to_string();
        let pp = macros::preprocess(&original_src);
        self.diagnostics.extend(pp.diagnostics);
        let file = if pp.source == original_src {
            file
        } else {
            let parsed = mty_syntax::parse(&pp.source);
            let root = mty_syntax::SyntaxNode::new_root(parsed.green);
            <mty_ast::File as mty_ast::AstNode>::cast(root).unwrap_or(file)
        };
        for node in file.0.children() {
            if let Some(item_id) = items::lower_item(&mut self, node) {
                self.package.top_level.push(item_id);
            }
        }
        (self.package, self.diagnostics)
    }

    pub fn alloc_type(&mut self, t: HirType) -> TypeId {
        self.package.types.alloc(t)
    }
    pub fn alloc_expr(&mut self, e: HirExpr) -> ExprId {
        self.package.exprs.alloc(e)
    }
    pub fn alloc_pat(&mut self, p: HirPat) -> PatId {
        self.package.pats.alloc(p)
    }
    pub fn alloc_block(&mut self, b: HirBlock) -> BlockId {
        self.package.blocks.alloc(b)
    }
}

pub fn span_of(n: &mty_syntax::SyntaxNode) -> SourceSpan {
    let r = n.text_range();
    SourceSpan {
        start: r.start().into(),
        end: r.end().into(),
    }
}
