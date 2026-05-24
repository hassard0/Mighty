//! Agent/protocol/supervisor lowering — full implementation lands in Task 22.
//!
//! These stubs exist so Task 21 (items/types/patterns) can link. They allocate
//! empty placeholders so the items dispatcher can route through them.

use crate::nodes::*;
use crate::ids::*;
use sdust_ast::{AgentDecl, ProtocolDecl, SupervisorDecl};
use super::LoweringCtx;

pub fn lower_agent(ctx: &mut LoweringCtx, _a: AgentDecl) -> AgentId {
    ctx.package.agents.alloc(HirAgent {
        name: String::new(),
        ctor_params: vec![],
        protocols: vec![],
        state: vec![],
        handlers: vec![],
        methods: vec![],
        span: SourceSpan { start: 0, end: 0 },
    })
}

pub fn lower_protocol(ctx: &mut LoweringCtx, _p: ProtocolDecl) -> ProtocolId {
    ctx.package.protocols.alloc(HirProtocol {
        name: String::new(),
        is_pub: false,
        version: None,
        composition: None,
        messages: vec![],
        span: SourceSpan { start: 0, end: 0 },
    })
}

pub fn lower_supervisor(ctx: &mut LoweringCtx, _s: SupervisorDecl) -> SupervisorId {
    ctx.package.supervisors.alloc(HirSupervisor {
        name: String::new(),
        strategy: String::new(),
        children: vec![],
        on_fail: vec![],
        span: SourceSpan { start: 0, end: 0 },
    })
}
