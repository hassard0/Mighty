//! Expression and block lowering (Task 22).

use crate::ids::*;
use crate::nodes::*;
use mty_ast::AstNode;
use mty_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use super::LoweringCtx;

pub fn lower_expr(ctx: &mut LoweringCtx, n: SyntaxNode) -> ExprId {
    let e = match n.kind() {
        SyntaxKind::LITERAL_EXPR => HirExpr::Literal(
            n.first_token()
                .as_ref()
                .map(lower_literal_token)
                .unwrap_or(HirLiteral::Bool(false)),
        ),
        SyntaxKind::PATH_EXPR => {
            let segs = path_segments(&n);
            // Collect generics from any GENERIC_ARG_LIST descendant (slice 2
            // attaches it to the final path segment via turbofish).
            let generics: Vec<TypeId> = n
                .descendants()
                .find(|d| d.kind() == SyntaxKind::GENERIC_ARG_LIST)
                .map(|gl| {
                    gl.children()
                        .filter(|c| c.kind() == SyntaxKind::GENERIC_ARG)
                        .filter_map(|g| g.children().find(|c| super::items::is_type_node(c.kind())))
                        .map(|t| super::types::lower_type(ctx, t))
                        .collect()
                })
                .unwrap_or_default();
            if generics.is_empty() {
                HirExpr::Path(segs)
            } else {
                HirExpr::PathGeneric {
                    segments: segs,
                    generics,
                }
            }
        }
        SyntaxKind::BINARY_EXPR => {
            let mut kids = child_exprs(&n);
            let op = lower_bin_op(&n);
            let lhs = if !kids.is_empty() {
                lower_expr(ctx, kids.remove(0))
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            let rhs = if !kids.is_empty() {
                lower_expr(ctx, kids.remove(0))
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            HirExpr::Binary { op, lhs, rhs }
        }
        SyntaxKind::UNARY_EXPR => {
            let op = lower_un_op(&n);
            let mut kids = child_exprs(&n);
            let rhs = if !kids.is_empty() {
                lower_expr(ctx, kids.remove(0))
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            HirExpr::Unary { op, rhs }
        }
        SyntaxKind::BORROW_EXPR => {
            let mutable = has_token(&n, SyntaxKind::MUT_KW);
            let inner = first_child_expr_id(ctx, &n);
            HirExpr::Borrow { mutable, inner }
        }
        SyntaxKind::MOVE_EXPR => HirExpr::Move(first_child_expr_id(ctx, &n)),
        SyntaxKind::SPAWN_EXPR => HirExpr::Spawn {
            is_task: has_token(&n, SyntaxKind::TASK_KW),
            inner: first_child_expr_id(ctx, &n),
        },
        SyntaxKind::CALL_EXPR => {
            let mut kids = child_exprs(&n);
            let callee = if !kids.is_empty() {
                lower_expr(ctx, kids.remove(0))
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            let args = lower_arg_list(ctx, &n);
            HirExpr::Call { callee, args }
        }
        SyntaxKind::METHOD_CALL_EXPR => {
            let mut kids = child_exprs(&n);
            let receiver = if !kids.is_empty() {
                lower_expr(ctx, kids.remove(0))
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            let method = first_name_text(&n);
            let args = lower_arg_list(ctx, &n);
            HirExpr::MethodCall {
                receiver,
                method,
                args,
            }
        }
        SyntaxKind::FIELD_EXPR => {
            // FIELD_EXPR with ARG_LIST really means MethodCall (parser shouldn't,
            // but historically some shapes are ambiguous). Distinguish by ARG_LIST.
            let mut kids = child_exprs(&n);
            let receiver = if !kids.is_empty() {
                lower_expr(ctx, kids.remove(0))
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            let name = first_name_text(&n);
            if has_child(&n, SyntaxKind::ARG_LIST) {
                let args = lower_arg_list(ctx, &n);
                HirExpr::MethodCall {
                    receiver,
                    method: name,
                    args,
                }
            } else {
                HirExpr::Field { receiver, name }
            }
        }
        SyntaxKind::INDEX_EXPR => {
            let mut kids = child_exprs(&n);
            let receiver = if !kids.is_empty() {
                lower_expr(ctx, kids.remove(0))
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            let idx = if !kids.is_empty() {
                lower_expr(ctx, kids.remove(0))
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            HirExpr::Index { receiver, idx }
        }
        SyntaxKind::SEND_EXPR => {
            let mut kids = child_exprs(&n);
            let target = if !kids.is_empty() {
                lower_expr(ctx, kids.remove(0))
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            let msg = first_name_text(&n);
            let args = lower_arg_list(ctx, &n);
            HirExpr::Send { target, msg, args }
        }
        SyntaxKind::ASK_EXPR => {
            let mut kids = child_exprs(&n);
            let target = if !kids.is_empty() {
                lower_expr(ctx, kids.remove(0))
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            let msg = first_name_text(&n);
            let args = lower_arg_list(ctx, &n);
            HirExpr::Ask { target, msg, args }
        }
        SyntaxKind::DEADLINE_EXPR => {
            let mut kids = child_exprs(&n);
            let inner = if !kids.is_empty() {
                lower_expr(ctx, kids.remove(0))
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            let dur = if !kids.is_empty() {
                lower_expr(ctx, kids.remove(0))
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            HirExpr::Deadline { inner, dur }
        }
        SyntaxKind::QUESTION_EXPR => HirExpr::Question(first_child_expr_id(ctx, &n)),
        SyntaxKind::IF_EXPR => {
            // IF_EXPR has two shapes:
            //   if cond { then } [else ...]
            //   if let pat = scrutinee { then } [else ...]
            // The distinguishing token is LET_KW; slice 2 represents the
            // second shape as HirExpr::IfLet.
            let has_let = n
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .any(|t| t.kind() == SyntaxKind::LET_KW);
            let mut kids: Vec<SyntaxNode> = n.children().collect();

            if has_let {
                // pat = first pattern child, scrutinee = first expr child,
                // then = first BLOCK, else_ = subsequent BLOCK or nested IF_EXPR.
                let pat_idx = kids
                    .iter()
                    .position(|c| super::patterns::is_pat_node(c.kind()));
                let pat = if let Some(i) = pat_idx {
                    super::patterns::lower_pat(ctx, kids.remove(i))
                } else {
                    ctx.alloc_pat(HirPat::Wildcard)
                };
                let scrut_idx = kids.iter().position(|c| is_expr_node(c.kind()));
                let scrutinee = if let Some(i) = scrut_idx {
                    lower_expr(ctx, kids.remove(i))
                } else {
                    ctx.alloc_expr(HirExpr::Error)
                };
                let then_idx = kids.iter().position(|c| c.kind() == SyntaxKind::BLOCK);
                let then = if let Some(i) = then_idx {
                    lower_block_node(ctx, kids.remove(i))
                } else {
                    ctx.alloc_block(HirBlock {
                        stmts: vec![],
                        tail: None,
                    })
                };
                let else_ = kids
                    .into_iter()
                    .find(|c| c.kind() == SyntaxKind::IF_EXPR || c.kind() == SyntaxKind::BLOCK)
                    .map(|c| {
                        if c.kind() == SyntaxKind::BLOCK {
                            let bid = lower_block_node(ctx, c);
                            ctx.alloc_expr(HirExpr::Block(bid))
                        } else {
                            lower_expr(ctx, c)
                        }
                    });
                return ctx.alloc_expr(HirExpr::IfLet {
                    pat,
                    scrutinee,
                    then,
                    else_,
                });
            }

            // Plain if/else.
            let cond_idx = kids.iter().position(|c| is_expr_node(c.kind()));
            let cond = if let Some(i) = cond_idx {
                lower_expr(ctx, kids.remove(i))
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            let then_idx = kids.iter().position(|c| c.kind() == SyntaxKind::BLOCK);
            let then = if let Some(i) = then_idx {
                lower_block_node(ctx, kids.remove(i))
            } else {
                ctx.alloc_block(HirBlock {
                    stmts: vec![],
                    tail: None,
                })
            };
            let else_ = kids
                .into_iter()
                .find(|c| c.kind() == SyntaxKind::IF_EXPR || c.kind() == SyntaxKind::BLOCK)
                .map(|c| {
                    if c.kind() == SyntaxKind::BLOCK {
                        let bid = lower_block_node(ctx, c);
                        ctx.alloc_expr(HirExpr::Block(bid))
                    } else {
                        lower_expr(ctx, c)
                    }
                });
            HirExpr::If { cond, then, else_ }
        }
        SyntaxKind::MATCH_EXPR => {
            let mut exprs: Vec<SyntaxNode> = child_exprs(&n);
            let scrutinee = if !exprs.is_empty() {
                lower_expr(ctx, exprs.remove(0))
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            let arms: Vec<HirMatchArm> = n
                .children()
                .filter(|c| c.kind() == SyntaxKind::MATCH_ARM)
                .map(|arm| lower_match_arm(ctx, arm))
                .collect();
            HirExpr::Match { scrutinee, arms }
        }
        SyntaxKind::FOR_EXPR => {
            // pat, iter expr, block body
            let pat = n
                .children()
                .find(|c| super::patterns::is_pat_node(c.kind()))
                .map(|p| super::patterns::lower_pat(ctx, p))
                .unwrap_or_else(|| ctx.alloc_pat(HirPat::Wildcard));
            let iter = n
                .children()
                .find(|c| is_expr_node(c.kind()))
                .map(|e| lower_expr(ctx, e))
                .unwrap_or_else(|| ctx.alloc_expr(HirExpr::Error));
            let body = n
                .children()
                .find(|c| c.kind() == SyntaxKind::BLOCK)
                .map(|b| lower_block_node(ctx, b))
                .unwrap_or_else(|| {
                    ctx.alloc_block(HirBlock {
                        stmts: vec![],
                        tail: None,
                    })
                });
            HirExpr::For { pat, iter, body }
        }
        SyntaxKind::WHILE_EXPR => {
            let cond = n
                .children()
                .find(|c| is_expr_node(c.kind()))
                .map(|e| lower_expr(ctx, e))
                .unwrap_or_else(|| ctx.alloc_expr(HirExpr::Error));
            let body = n
                .children()
                .find(|c| c.kind() == SyntaxKind::BLOCK)
                .map(|b| lower_block_node(ctx, b))
                .unwrap_or_else(|| {
                    ctx.alloc_block(HirBlock {
                        stmts: vec![],
                        tail: None,
                    })
                });
            HirExpr::While { cond, body }
        }
        SyntaxKind::LOOP_EXPR => {
            let body = n
                .children()
                .find(|c| c.kind() == SyntaxKind::BLOCK)
                .map(|b| lower_block_node(ctx, b))
                .unwrap_or_else(|| {
                    ctx.alloc_block(HirBlock {
                        stmts: vec![],
                        tail: None,
                    })
                });
            HirExpr::Loop { body }
        }
        SyntaxKind::RETURN_EXPR => {
            let inner = n
                .children()
                .find(|c| is_expr_node(c.kind()))
                .map(|c| lower_expr(ctx, c));
            HirExpr::Return(inner)
        }
        SyntaxKind::BREAK_EXPR => {
            // `break <value>?` — unlabelled in v0.5. The value, when
            // present, becomes the value of the enclosing `loop` expr.
            let inner = n
                .children()
                .find(|c| is_expr_node(c.kind()))
                .map(|c| lower_expr(ctx, c));
            HirExpr::Break(inner)
        }
        SyntaxKind::CONTINUE_EXPR => HirExpr::Continue,
        SyntaxKind::BLOCK => HirExpr::Block(lower_block_node(ctx, n)),
        SyntaxKind::TUPLE_EXPR => HirExpr::Tuple(
            child_exprs(&n)
                .into_iter()
                .map(|c| lower_expr(ctx, c))
                .collect(),
        ),
        SyntaxKind::ARRAY_EXPR => HirExpr::Array(
            child_exprs(&n)
                .into_iter()
                .map(|c| lower_expr(ctx, c))
                .collect(),
        ),
        SyntaxKind::STRUCT_EXPR => {
            // STRUCT_EXPR: starts with PATH_EXPR (path), then STRUCT_FIELD_EXPR children.
            let path = n
                .children()
                .find(|c| c.kind() == SyntaxKind::PATH_EXPR)
                .map(|p| path_segments(&p))
                .unwrap_or_default();
            let fields: Vec<(String, ExprId)> = n
                .children()
                .filter(|c| c.kind() == SyntaxKind::STRUCT_FIELD_EXPR)
                .map(|f| {
                    let name = f
                        .children()
                        .find_map(mty_ast::Name::cast)
                        .map(|nm| nm.text())
                        .unwrap_or_default();
                    let value = f
                        .children()
                        .find(|c| is_expr_node(c.kind()))
                        .map(|e| lower_expr(ctx, e))
                        .unwrap_or_else(|| {
                            // Shorthand: `Foo { x }` — resynthesize as path-expr referring to `x`.
                            ctx.alloc_expr(HirExpr::Path(vec![name.clone()]))
                        });
                    (name, value)
                })
                .collect();
            HirExpr::Struct { path, fields }
        }
        SyntaxKind::MAP_EXPR => {
            let entries: Vec<(ExprId, ExprId)> = n
                .children()
                .filter(|c| c.kind() == SyntaxKind::MAP_ENTRY)
                .map(|entry| {
                    // Key is the first Name child (lowered as Path with one segment),
                    // value is the first expression child.
                    let key_name = entry
                        .children()
                        .find_map(mty_ast::Name::cast)
                        .map(|n| n.text())
                        .unwrap_or_default();
                    let key = ctx.alloc_expr(HirExpr::Path(vec![key_name]));
                    let value = entry
                        .children()
                        .find(|c| is_expr_node(c.kind()))
                        .map(|e| lower_expr(ctx, e))
                        .unwrap_or_else(|| ctx.alloc_expr(HirExpr::Error));
                    (key, value)
                })
                .collect();
            HirExpr::Map(entries)
        }
        SyntaxKind::HTML_EXPR => {
            let text = n
                .first_token()
                .map(|t| t.text().to_string())
                .unwrap_or_default();
            HirExpr::HtmlTemplate(text)
        }
        SyntaxKind::UNSAFE_BLOCK => {
            let body = n
                .children()
                .find(|c| c.kind() == SyntaxKind::BLOCK)
                .map(|b| lower_block_node(ctx, b))
                .unwrap_or_else(|| {
                    ctx.alloc_block(HirBlock {
                        stmts: vec![],
                        tail: None,
                    })
                });
            HirExpr::Unsafe(body)
        }
        SyntaxKind::ARENA_BLOCK => {
            let name = n
                .children()
                .find_map(mty_ast::Name::cast)
                .map(|n| n.text())
                .unwrap_or_default();
            // Body is either an inline expression (short form `arena name: expr`)
            // or a BLOCK (`arena name { ... }`).
            let body = if let Some(block) = n.children().find(|c| c.kind() == SyntaxKind::BLOCK) {
                let bid = lower_block_node(ctx, block);
                ctx.alloc_expr(HirExpr::Block(bid))
            } else if let Some(e) = n.children().find(|c| is_expr_node(c.kind())) {
                lower_expr(ctx, e)
            } else {
                ctx.alloc_expr(HirExpr::Error)
            };
            HirExpr::Arena { name, body }
        }
        SyntaxKind::TASK_SCOPE => {
            // The first expression child (if any) before the BLOCK is the deadline
            // (introduced by `@`). If only a BLOCK is present, deadline is None.
            let mut deadline = None;
            for c in n.children() {
                if c.kind() == SyntaxKind::BLOCK {
                    break;
                }
                if is_expr_node(c.kind()) {
                    deadline = Some(lower_expr(ctx, c));
                    break;
                }
            }
            let body = n
                .children()
                .find(|c| c.kind() == SyntaxKind::BLOCK)
                .map(|b| lower_block_node(ctx, b))
                .unwrap_or_else(|| {
                    ctx.alloc_block(HirBlock {
                        stmts: vec![],
                        tail: None,
                    })
                });
            HirExpr::TaskScope { deadline, body }
        }
        SyntaxKind::BUDGET_BLOCK => {
            let entries: Vec<(String, ExprId)> = n
                .children()
                .filter(|c| c.kind() == SyntaxKind::BUDGET_ENTRY)
                .map(|entry| {
                    let name = entry
                        .children()
                        .find_map(mty_ast::Name::cast)
                        .map(|n| n.text())
                        .unwrap_or_default();
                    let value = entry
                        .children()
                        .find(|c| is_expr_node(c.kind()))
                        .map(|e| lower_expr(ctx, e))
                        .unwrap_or_else(|| ctx.alloc_expr(HirExpr::Error));
                    (name, value)
                })
                .collect();
            // The body is whatever follows the entries: either a BLOCK or an expression.
            // Find the last block or expression child that *isn't* a BUDGET_ENTRY.
            let body = n
                .children()
                .filter(|c| c.kind() != SyntaxKind::BUDGET_ENTRY)
                .find(|c| c.kind() == SyntaxKind::BLOCK || is_expr_node(c.kind()))
                .map(|c| {
                    if c.kind() == SyntaxKind::BLOCK {
                        let bid = lower_block_node(ctx, c);
                        ctx.alloc_expr(HirExpr::Block(bid))
                    } else {
                        lower_expr(ctx, c)
                    }
                })
                .unwrap_or_else(|| ctx.alloc_expr(HirExpr::Error));
            HirExpr::Budget { entries, body }
        }
        SyntaxKind::SANDBOX_BLOCK => {
            let name = n
                .children()
                .find_map(mty_ast::Name::cast)
                .map(|n| n.text())
                .unwrap_or_default();
            let entries: Vec<(Vec<String>, ExprId)> = n
                .children()
                .filter(|c| c.kind() == SyntaxKind::SANDBOX_ENTRY)
                .map(|entry| {
                    let path: Vec<String> = entry
                        .children()
                        .find(|c| c.kind() == SyntaxKind::PATH)
                        .map(|p| path_text_segments(&p))
                        .unwrap_or_default();
                    let value = entry
                        .children()
                        .find(|c| is_expr_node(c.kind()))
                        .map(|e| lower_expr(ctx, e))
                        .unwrap_or_else(|| ctx.alloc_expr(HirExpr::Error));
                    (path, value)
                })
                .collect();
            let body = n
                .children()
                .find(|c| c.kind() == SyntaxKind::BLOCK)
                .map(|b| lower_block_node(ctx, b))
                .unwrap_or_else(|| {
                    ctx.alloc_block(HirBlock {
                        stmts: vec![],
                        tail: None,
                    })
                });
            HirExpr::Sandbox {
                name,
                entries,
                body,
            }
        }
        SyntaxKind::DETACH_EXPR => HirExpr::Detach(first_child_expr_id(ctx, &n)),
        SyntaxKind::JOIN_EXPR => HirExpr::Join(first_child_expr_id(ctx, &n)),
        SyntaxKind::CAST_EXPR => {
            let lhs = first_child_expr_id(ctx, &n);
            let ty = n
                .children()
                .find(|c| super::items::is_type_node(c.kind()))
                .map(|t| super::types::lower_type(ctx, t))
                .unwrap_or_else(|| ctx.alloc_type(HirType::Unknown));
            HirExpr::Cast { lhs, ty }
        }
        SyntaxKind::LAMBDA_EXPR => {
            // Params from FN_PARAM_LIST -> FN_PARAM children.
            let params: Vec<HirParam> = n
                .children()
                .find(|c| c.kind() == SyntaxKind::FN_PARAM_LIST)
                .map(|pl| {
                    pl.children()
                        .filter(|c| c.kind() == SyntaxKind::FN_PARAM)
                        .map(|param| {
                            let name = param
                                .children()
                                .find_map(mty_ast::Name::cast)
                                .map(|nm| nm.text())
                                .unwrap_or_default();
                            let ty = param
                                .children()
                                .find(|c| super::items::is_type_node(c.kind()))
                                .map(|t| super::types::lower_type(ctx, t));
                            let start: u32 = u32::from(param.text_range().start());
                            let end: u32 = u32::from(param.text_range().end());
                            HirParam {
                                name,
                                ty,
                                span: SourceSpan { start, end },
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let ret = n
                .children()
                .find(|c| c.kind() == SyntaxKind::RET_TYPE)
                .and_then(|rt| rt.children().find(|c| super::items::is_type_node(c.kind())))
                .map(|t| super::types::lower_type(ctx, t));
            let body = n
                .children()
                .find(|c| c.kind() == SyntaxKind::BLOCK)
                .map(|b| lower_block_node(ctx, b))
                .unwrap_or_else(|| {
                    ctx.alloc_block(HirBlock {
                        stmts: vec![],
                        tail: None,
                    })
                });
            HirExpr::Lambda { params, ret, body }
        }
        SyntaxKind::RUN_EXPR => HirExpr::Run(first_child_expr_id(ctx, &n)),
        _ => HirExpr::Error,
    };
    ctx.alloc_expr(e)
}

pub fn lower_block(ctx: &mut LoweringCtx, b: mty_ast::Block) -> BlockId {
    lower_block_inner(ctx, b.0)
}

pub fn lower_block_node(ctx: &mut LoweringCtx, n: SyntaxNode) -> BlockId {
    lower_block_inner(ctx, n)
}

fn lower_block_inner(ctx: &mut LoweringCtx, n: SyntaxNode) -> BlockId {
    let mut stmts: Vec<HirStmt> = Vec::new();
    let mut tail: Option<ExprId> = None;

    // Collect statement-level children in order.
    let kids: Vec<SyntaxNode> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::LET_STMT || c.kind() == SyntaxKind::EXPR_STMT)
        .collect();
    let last_idx = kids.len().saturating_sub(1);

    for (i, child) in kids.into_iter().enumerate() {
        match child.kind() {
            SyntaxKind::LET_STMT => {
                let pat = child
                    .children()
                    .find(|c| super::patterns::is_pat_node(c.kind()))
                    .map(|p| super::patterns::lower_pat(ctx, p))
                    .unwrap_or_else(|| ctx.alloc_pat(HirPat::Wildcard));
                let ty = child
                    .children()
                    .find(|c| super::items::is_type_node(c.kind()))
                    .map(|t| super::types::lower_type(ctx, t));
                let init = child
                    .children()
                    .find(|c| is_expr_node(c.kind()))
                    .map(|e| lower_expr(ctx, e));
                // `let mut <pat> ...` marks the binding(s) as mutable.
                let mutable = child
                    .children_with_tokens()
                    .filter_map(|c| c.into_token())
                    .any(|t| t.kind() == SyntaxKind::MUT_KW);
                stmts.push(HirStmt::Let {
                    pat,
                    ty,
                    init,
                    mutable,
                });
            }
            SyntaxKind::EXPR_STMT => {
                let has_semi = has_token(&child, SyntaxKind::SEMI);
                let inner = child
                    .children()
                    .find(|c| is_expr_node(c.kind()))
                    .map(|e| lower_expr(ctx, e));
                let inner = inner.unwrap_or_else(|| ctx.alloc_expr(HirExpr::Error));
                // The last EXPR_STMT without a trailing semi becomes the tail.
                if i == last_idx && !has_semi {
                    tail = Some(inner);
                } else {
                    stmts.push(HirStmt::Expr(inner));
                }
            }
            _ => {}
        }
    }

    ctx.alloc_block(HirBlock { stmts, tail })
}

fn lower_match_arm(ctx: &mut LoweringCtx, arm: SyntaxNode) -> HirMatchArm {
    let pat = arm
        .children()
        .find(|c| super::patterns::is_pat_node(c.kind()))
        .map(|p| super::patterns::lower_pat(ctx, p))
        .unwrap_or_else(|| ctx.alloc_pat(HirPat::Wildcard));
    let guard = arm
        .children()
        .find(|c| c.kind() == SyntaxKind::MATCH_GUARD)
        .and_then(|g| g.children().find(|c| is_expr_node(c.kind())))
        .map(|e| lower_expr(ctx, e));
    // The body is the first expression child after the pattern (and possibly the
    // MATCH_GUARD). We accept either a BLOCK or any expression node here.
    let body_node = arm
        .children()
        .filter(|c| is_expr_node(c.kind()) || c.kind() == SyntaxKind::BLOCK)
        .last();
    let body = body_node
        .map(|c| {
            if c.kind() == SyntaxKind::BLOCK {
                let bid = lower_block_node(ctx, c);
                ctx.alloc_expr(HirExpr::Block(bid))
            } else {
                lower_expr(ctx, c)
            }
        })
        .unwrap_or_else(|| ctx.alloc_expr(HirExpr::Error));
    HirMatchArm { pat, guard, body }
}

// ---- helpers ----

pub fn is_expr_node(k: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(
        k,
        LITERAL_EXPR
            | PATH_EXPR
            | BINARY_EXPR
            | UNARY_EXPR
            | BORROW_EXPR
            | MOVE_EXPR
            | SPAWN_EXPR
            | CALL_EXPR
            | METHOD_CALL_EXPR
            | FIELD_EXPR
            | INDEX_EXPR
            | SEND_EXPR
            | ASK_EXPR
            | DEADLINE_EXPR
            | QUESTION_EXPR
            | IF_EXPR
            | MATCH_EXPR
            | FOR_EXPR
            | WHILE_EXPR
            | LOOP_EXPR
            | RETURN_EXPR
            | BREAK_EXPR
            | CONTINUE_EXPR
            | TUPLE_EXPR
            | ARRAY_EXPR
            | STRUCT_EXPR
            | MAP_EXPR
            | HTML_EXPR
            | UNSAFE_BLOCK
            | ARENA_BLOCK
            | TASK_SCOPE
            | BUDGET_BLOCK
            | SANDBOX_BLOCK
            | DETACH_EXPR
            | JOIN_EXPR
            | LAMBDA_EXPR
            | RUN_EXPR
            | CAST_EXPR
    )
}

fn child_exprs(n: &SyntaxNode) -> Vec<SyntaxNode> {
    n.children().filter(|c| is_expr_node(c.kind())).collect()
}

fn first_child_expr_id(ctx: &mut LoweringCtx, n: &SyntaxNode) -> ExprId {
    n.children()
        .find(|c| is_expr_node(c.kind()))
        .map(|c| lower_expr(ctx, c))
        .unwrap_or_else(|| ctx.alloc_expr(HirExpr::Error))
}

fn has_token(n: &SyntaxNode, kind: SyntaxKind) -> bool {
    n.children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == kind)
}

fn has_child(n: &SyntaxNode, kind: SyntaxKind) -> bool {
    n.children().any(|c| c.kind() == kind)
}

fn first_name_text(n: &SyntaxNode) -> String {
    n.children()
        .find_map(mty_ast::Name::cast)
        .map(|nm| nm.text())
        .unwrap_or_default()
}

fn path_segments(n: &SyntaxNode) -> Vec<String> {
    // For a PATH_EXPR (or any node containing a PATH), collect each NameRef's text.
    n.descendants()
        .filter_map(mty_ast::NameRef::cast)
        .map(|nr| {
            nr.0.first_token()
                .map(|t| t.text().to_string())
                .unwrap_or_default()
        })
        .collect()
}

fn path_text_segments(path: &SyntaxNode) -> Vec<String> {
    path.descendants()
        .filter_map(mty_ast::NameRef::cast)
        .map(|nr| {
            nr.0.first_token()
                .map(|t| t.text().to_string())
                .unwrap_or_default()
        })
        .collect()
}

fn lower_arg_list(ctx: &mut LoweringCtx, n: &SyntaxNode) -> Vec<HirArg> {
    let Some(arg_list) = n.children().find(|c| c.kind() == SyntaxKind::ARG_LIST) else {
        return vec![];
    };
    arg_list
        .children()
        .filter(|c| c.kind() == SyntaxKind::ARG || c.kind() == SyntaxKind::NAMED_ARG)
        .map(|arg| {
            let name = if arg.kind() == SyntaxKind::NAMED_ARG {
                arg.children()
                    .find_map(mty_ast::Name::cast)
                    .map(|nm| nm.text())
            } else {
                None
            };
            let value = arg
                .children()
                .find(|c| is_expr_node(c.kind()))
                .map(|e| lower_expr(ctx, e))
                .unwrap_or_else(|| ctx.alloc_expr(HirExpr::Error));
            HirArg { name, value }
        })
        .collect()
}

fn lower_bin_op(n: &SyntaxNode) -> BinOp {
    // Find the operator token between LHS and RHS expression children.
    // Strategy: walk children-with-tokens; find the first non-trivia token that
    // appears *after* the first expression child.
    let mut seen_first_expr = false;
    for el in n.children_with_tokens() {
        if let Some(child) = el.as_node() {
            if is_expr_node(child.kind()) {
                if seen_first_expr {
                    break;
                }
                seen_first_expr = true;
            }
        } else if let Some(tok) = el.as_token() {
            if !seen_first_expr || tok.kind().is_trivia() {
                continue;
            }
            if let Some(op) = bin_op_from_kind(tok.kind()) {
                return op;
            }
        }
    }
    BinOp::Add
}

fn bin_op_from_kind(k: SyntaxKind) -> Option<BinOp> {
    use SyntaxKind::*;
    let op = match k {
        PLUS => BinOp::Add,
        MINUS => BinOp::Sub,
        STAR => BinOp::Mul,
        SLASH => BinOp::Div,
        PERCENT => BinOp::Rem,
        AMP => BinOp::BitAnd,
        PIPE => BinOp::BitOr,
        CARET => BinOp::BitXor,
        SHL => BinOp::Shl,
        SHR => BinOp::Shr,
        EQ_EQ => BinOp::Eq,
        BANG_EQ => BinOp::Ne,
        LT => BinOp::Lt,
        LT_EQ => BinOp::Le,
        GT => BinOp::Gt,
        GT_EQ => BinOp::Ge,
        AMP_AMP => BinOp::And,
        PIPE_PIPE => BinOp::Or,
        DOT_DOT => BinOp::Range,
        DOT_DOT_EQ => BinOp::RangeEq,
        EQ => BinOp::Assign,
        PLUS_EQ => BinOp::AssignAdd,
        MINUS_EQ => BinOp::AssignSub,
        STAR_EQ => BinOp::AssignMul,
        SLASH_EQ => BinOp::AssignDiv,
        PERCENT_EQ => BinOp::AssignRem,
        AMP_EQ => BinOp::AssignBitAnd,
        PIPE_EQ => BinOp::AssignBitOr,
        CARET_EQ => BinOp::AssignBitXor,
        SHL_EQ => BinOp::AssignShl,
        SHR_EQ => BinOp::AssignShr,
        _ => return None,
    };
    Some(op)
}

fn lower_un_op(n: &SyntaxNode) -> UnOp {
    // Find the leading operator token before the first expression child.
    for el in n.children_with_tokens() {
        if let Some(tok) = el.as_token() {
            if tok.kind().is_trivia() {
                continue;
            }
            return match tok.kind() {
                SyntaxKind::MINUS => UnOp::Neg,
                SyntaxKind::BANG => UnOp::Not,
                SyntaxKind::STAR => UnOp::Deref,
                _ => UnOp::Neg,
            };
        } else if let Some(child) = el.as_node() {
            if is_expr_node(child.kind()) {
                break;
            }
        }
    }
    UnOp::Neg
}

pub fn lower_literal_token(tok: &SyntaxToken) -> HirLiteral {
    match tok.kind() {
        SyntaxKind::INT_LITERAL => parse_int_literal(tok.text()),
        SyntaxKind::FLOAT_LITERAL => parse_float_literal(tok.text()),
        SyntaxKind::STRING_LITERAL => {
            let raw = tok.text();
            let inner = raw
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(raw);
            HirLiteral::Str(decode_str_escapes(inner))
        }
        SyntaxKind::CHAR_LITERAL => {
            let raw = tok.text();
            let inner = raw
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
                .unwrap_or(raw);
            let decoded = decode_str_escapes(inner);
            HirLiteral::Char(decoded.chars().next().unwrap_or('\0'))
        }
        SyntaxKind::TRUE_KW => HirLiteral::Bool(true),
        SyntaxKind::FALSE_KW => HirLiteral::Bool(false),
        SyntaxKind::DURATION_LITERAL => parse_duration_literal(tok.text()),
        SyntaxKind::SIZE_LITERAL => parse_size_literal(tok.text()),
        _ => HirLiteral::Bool(false),
    }
}

fn parse_int_literal(text: &str) -> HirLiteral {
    // Strip an optional i/u/f suffix.
    let (digits, suffix) = split_int_suffix(text);
    let cleaned: String = digits.chars().filter(|c| *c != '_').collect();
    let v: i128 = cleaned.parse().unwrap_or(0);
    HirLiteral::Int(v, suffix.map(|s| s.to_string()))
}

fn split_int_suffix(s: &str) -> (&str, Option<&str>) {
    // Find the start of the suffix: i8/i16/.../u128/f32/f64
    for marker in ["i", "u", "f"] {
        if let Some(idx) = s.find(marker) {
            // The character before idx (if any) must be a digit or underscore.
            // (For pure-digit prefixes this is always satisfied unless idx == 0.)
            if idx > 0 {
                let (digits, suffix) = s.split_at(idx);
                return (digits, Some(suffix));
            }
        }
    }
    (s, None)
}

fn parse_float_literal(text: &str) -> HirLiteral {
    let (digits, suffix) = if let Some(idx) = text.find('f') {
        if idx > 0 {
            (&text[..idx], Some(&text[idx..]))
        } else {
            (text, None)
        }
    } else {
        (text, None)
    };
    let cleaned: String = digits.chars().filter(|c| *c != '_').collect();
    let v: f64 = cleaned.parse().unwrap_or(0.0);
    HirLiteral::Float(v, suffix.map(|s| s.to_string()))
}

fn parse_duration_literal(text: &str) -> HirLiteral {
    let split = text
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(text.len());
    let (num, unit) = text.split_at(split);
    let value: u64 = num.parse().unwrap_or(0);
    HirLiteral::Duration {
        value,
        unit: unit.to_string(),
    }
}

fn parse_size_literal(text: &str) -> HirLiteral {
    let split = text
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(text.len());
    let (num, unit) = text.split_at(split);
    let value: u64 = num.parse().unwrap_or(0);
    HirLiteral::Size {
        value,
        unit: unit.to_string(),
    }
}

fn decode_str_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                match next {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    '\\' => out.push('\\'),
                    '"' => out.push('"'),
                    '\'' => out.push('\''),
                    '0' => out.push('\0'),
                    other => {
                        out.push('\\');
                        out.push(other);
                    }
                }
            } else {
                out.push('\\');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Slice 5: lower a top-level SANDBOX_BLOCK into HirTopSandbox.
pub fn lower_top_sandbox(ctx: &mut LoweringCtx, n: SyntaxNode) -> HirTopSandbox {
    let span = super::span_of(&n);
    let name = n
        .children()
        .find_map(mty_ast::Name::cast)
        .map(|x| x.text())
        .unwrap_or_default();
    let entries: Vec<(Vec<String>, ExprId)> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::SANDBOX_ENTRY)
        .map(|entry| {
            let path: Vec<String> = entry
                .children()
                .find(|c| c.kind() == SyntaxKind::PATH)
                .map(|p| path_text_segments(&p))
                .unwrap_or_default();
            let value = entry
                .children()
                .find(|c| is_expr_node(c.kind()))
                .map(|e| lower_expr(ctx, e))
                .unwrap_or_else(|| ctx.alloc_expr(HirExpr::Error));
            (path, value)
        })
        .collect();
    let body = n
        .children()
        .find(|c| c.kind() == SyntaxKind::BLOCK)
        .map(|b| lower_block_node(ctx, b))
        .unwrap_or_else(|| {
            ctx.alloc_block(HirBlock {
                stmts: vec![],
                tail: None,
            })
        });
    HirTopSandbox {
        name,
        entries,
        body,
        span,
    }
}
