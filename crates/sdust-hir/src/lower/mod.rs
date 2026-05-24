//! HIR lowering — populated by Tasks 21, 22.

use crate::nodes::*;
use crate::ids::*;
use sdust_diagnostics::Diagnostic;

pub mod items;
pub mod types;
pub mod patterns;
pub mod exprs;
pub mod agents;

pub struct LoweringCtx {
    pub package: Package,
    pub diagnostics: Vec<Diagnostic>,
}

impl Default for LoweringCtx {
    fn default() -> Self { Self::new() }
}

impl LoweringCtx {
    pub fn new() -> Self { Self { package: Package::default(), diagnostics: vec![] } }

    pub fn lower_file(mut self, file: sdust_ast::File) -> (Package, Vec<Diagnostic>) {
        for node in file.0.children() {
            if let Some(item_id) = items::lower_item(&mut self, node) {
                self.package.top_level.push(item_id);
            }
        }
        (self.package, self.diagnostics)
    }

    pub fn alloc_type(&mut self, t: HirType) -> TypeId { self.package.types.alloc(t) }
    pub fn alloc_expr(&mut self, e: HirExpr) -> ExprId { self.package.exprs.alloc(e) }
    pub fn alloc_pat(&mut self, p: HirPat)  -> PatId  { self.package.pats.alloc(p) }
    pub fn alloc_block(&mut self, b: HirBlock) -> BlockId { self.package.blocks.alloc(b) }
}

pub fn span_of(n: &sdust_syntax::SyntaxNode) -> SourceSpan {
    let r = n.text_range();
    SourceSpan { start: r.start().into(), end: r.end().into() }
}
