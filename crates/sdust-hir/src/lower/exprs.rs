//! Expression and block lowering — full implementation lands in Task 22.
//!
//! These stubs exist so Task 21 (items/types/patterns) can link. They allocate
//! empty/placeholder nodes so that downstream code can call them safely.

use crate::nodes::*;
use crate::ids::*;
use sdust_syntax::{SyntaxNode, SyntaxToken};
use super::LoweringCtx;

pub fn lower_block(ctx: &mut LoweringCtx, _b: sdust_ast::Block) -> BlockId {
    ctx.package.blocks.alloc(HirBlock { stmts: vec![], tail: None })
}

pub fn lower_expr(ctx: &mut LoweringCtx, _n: SyntaxNode) -> ExprId {
    ctx.package.exprs.alloc(HirExpr::Error)
}

pub fn lower_literal_token(_tok: &SyntaxToken) -> HirLiteral {
    HirLiteral::Bool(false)
}

pub fn lower_block_node(ctx: &mut LoweringCtx, _b: SyntaxNode) -> BlockId {
    ctx.package.blocks.alloc(HirBlock { stmts: vec![], tail: None })
}
