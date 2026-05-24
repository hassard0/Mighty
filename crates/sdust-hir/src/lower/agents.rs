//! Agent/protocol/supervisor lowering (Task 22).

use crate::ids::*;
use crate::nodes::*;
use sdust_ast::{AgentDecl, AstNode, ProtocolDecl, SupervisorDecl};
use sdust_syntax::SyntaxKind;

use super::{span_of, LoweringCtx};

pub fn lower_agent(ctx: &mut LoweringCtx, a: AgentDecl) -> AgentId {
    let name = a.name().map(|n| n.text()).unwrap_or_default();

    let ctor_params: Vec<String> = a
        .ctor_params()
        .map(|cp| {
            cp.0.children()
                .filter_map(sdust_ast::Name::cast)
                .map(|n| n.text())
                .collect()
        })
        .unwrap_or_default();

    let protocols: Vec<TypeId> = a
        .protocols()
        .map(|pl| {
            pl.0.children()
                .filter(|c| super::items::is_type_node(c.kind()))
                .map(|tn| super::types::lower_type(ctx, tn))
                .collect()
        })
        .unwrap_or_default();

    let state: Vec<HirAgentState> = a
        .state_fields()
        .map(|sf| {
            let nm =
                sf.0.children()
                    .find_map(sdust_ast::Name::cast)
                    .map(|n| n.text())
                    .unwrap_or_default();
            let ty =
                sf.0.children()
                    .find(|c| super::items::is_type_node(c.kind()))
                    .map(|tn| super::types::lower_type(ctx, tn));
            let init =
                sf.0.children()
                    .find(|c| super::exprs::is_expr_node(c.kind()))
                    .map(|c| super::exprs::lower_expr(ctx, c));
            HirAgentState {
                name: nm,
                ty,
                init,
                span: span_of(&sf.0),
            }
        })
        .collect();

    let handlers: Vec<HirOnHandler> = a
        .handlers()
        .map(|h| {
            // First Name child = message name; subsequent Name children = parameter names.
            let mut names = h.0.children().filter_map(sdust_ast::Name::cast);
            let message = names.next().map(|n| n.text()).unwrap_or_default();
            let params: Vec<String> = names.map(|n| n.text()).collect();

            // Body is either a BLOCK child or a single expression (short form
            // `on Msg(args) -> expr`).
            let body = if let Some(block) = h.0.children().find(|c| c.kind() == SyntaxKind::BLOCK) {
                super::exprs::lower_block_node(ctx, block)
            } else if let Some(expr_node) =
                h.0.children()
                    .find(|c| super::exprs::is_expr_node(c.kind()))
            {
                let tail = super::exprs::lower_expr(ctx, expr_node);
                ctx.alloc_block(HirBlock {
                    stmts: vec![],
                    tail: Some(tail),
                })
            } else {
                ctx.alloc_block(HirBlock {
                    stmts: vec![],
                    tail: None,
                })
            };

            HirOnHandler {
                message,
                params,
                body,
                span: span_of(&h.0),
            }
        })
        .collect();

    // Collect agent methods: any FN_DECL appearing inside the agent body.
    let methods: Vec<FnId> =
        a.0.descendants()
            .filter_map(sdust_ast::FnDecl::cast)
            .map(|f| super::items::lower_fn_public(ctx, f))
            .collect();

    let ha = HirAgent {
        name,
        ctor_params,
        protocols,
        state,
        handlers,
        methods,
        span: span_of(&a.0),
    };
    ctx.package.agents.alloc(ha)
}

pub fn lower_protocol(ctx: &mut LoweringCtx, p: ProtocolDecl) -> ProtocolId {
    let name =
        p.0.children()
            .find_map(sdust_ast::Name::cast)
            .map(|n| n.text())
            .unwrap_or_default();

    let messages: Vec<HirProtocolMsg> =
        p.0.descendants()
            .filter_map(sdust_ast::ProtocolMsg::cast)
            .map(|m| {
                let msg_name =
                    m.0.children()
                        .find_map(sdust_ast::Name::cast)
                        .map(|n| n.text())
                        .unwrap_or_default();
                let params: Vec<HirParam> =
                    m.0.children()
                        .filter_map(sdust_ast::FnParam::cast)
                        .map(|fp| {
                            let pname =
                                fp.0.children()
                                    .find_map(sdust_ast::Name::cast)
                                    .map(|n| n.text())
                                    .unwrap_or_default();
                            let ty =
                                fp.0.children()
                                    .find(|c| super::items::is_type_node(c.kind()))
                                    .map(|tn| super::types::lower_type(ctx, tn));
                            HirParam {
                                name: pname,
                                ty,
                                span: span_of(&fp.0),
                            }
                        })
                        .collect();
                // Reply type is the last *direct* type child (after the parameter list,
                // which itself contains nested types but those are not direct children
                // of PROTOCOL_MSG).
                let reply =
                    m.0.children()
                        .filter(|c| super::items::is_type_node(c.kind()))
                        .last()
                        .map(|tn| super::types::lower_type(ctx, tn));
                HirProtocolMsg {
                    name: msg_name,
                    params,
                    reply,
                    span: span_of(&m.0),
                }
            })
            .collect();

    let hp = HirProtocol {
        name,
        is_pub: super::items::has_visibility(&p.0),
        version: None,
        composition: None,
        messages,
        span: span_of(&p.0),
    };
    ctx.package.protocols.alloc(hp)
}

pub fn lower_supervisor(ctx: &mut LoweringCtx, s: SupervisorDecl) -> SupervisorId {
    let name =
        s.0.children()
            .find_map(sdust_ast::Name::cast)
            .map(|n| n.text())
            .unwrap_or_default();

    // The strategy, if present, is encoded as the second top-level Name child of
    // the SUPERVISOR_DECL (immediately following the supervisor's own name) or
    // shows up as a named arg `strategy: one_for_one`. Fall back to "one_for_one".
    let strategy =
        s.0.children()
            .filter_map(sdust_ast::Name::cast)
            .nth(1)
            .map(|n| n.text())
            .unwrap_or_else(|| "one_for_one".to_string());

    let children: Vec<(String, ExprId)> =
        s.0.children()
            .filter(|c| c.kind() == SyntaxKind::SUP_CHILD)
            .map(|c| {
                let nm = c
                    .children()
                    .find_map(sdust_ast::Name::cast)
                    .map(|n| n.text())
                    .unwrap_or_default();
                let init = c
                    .children()
                    .find(|child| super::exprs::is_expr_node(child.kind()))
                    .map(|child| super::exprs::lower_expr(ctx, child))
                    .unwrap_or_else(|| ctx.alloc_expr(HirExpr::Error));
                (nm, init)
            })
            .collect();

    let hs = HirSupervisor {
        name,
        strategy,
        children,
        on_fail: vec![],
        span: span_of(&s.0),
    };
    ctx.package.supervisors.alloc(hs)
}
