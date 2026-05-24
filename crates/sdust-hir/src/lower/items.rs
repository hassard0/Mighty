use super::{span_of, LoweringCtx};
use crate::ids::*;
use crate::nodes::*;
use sdust_ast::{
    AgentDecl, AstNode, EnumDecl, FnDecl, ModDecl, ProtocolDecl, StructDecl, SupervisorDecl,
    TypeAlias, UseDecl,
};
use sdust_syntax::{SyntaxKind, SyntaxNode};

pub fn lower_item(ctx: &mut LoweringCtx, node: SyntaxNode) -> Option<ItemId> {
    let item = match node.kind() {
        SyntaxKind::FN_DECL => Item::Fn(lower_fn(ctx, FnDecl::cast(node)?)),
        SyntaxKind::STRUCT_DECL => Item::Struct(lower_struct(ctx, StructDecl::cast(node)?)),
        SyntaxKind::ENUM_DECL => Item::Enum(lower_enum(ctx, EnumDecl::cast(node)?)),
        SyntaxKind::TYPE_ALIAS => Item::TypeAlias(lower_type_alias(ctx, TypeAlias::cast(node)?)),
        SyntaxKind::AGENT_DECL => {
            Item::Agent(super::agents::lower_agent(ctx, AgentDecl::cast(node)?))
        }
        SyntaxKind::PROTOCOL_DECL => Item::Protocol(super::agents::lower_protocol(
            ctx,
            ProtocolDecl::cast(node)?,
        )),
        SyntaxKind::SUPERVISOR_DECL => Item::Supervisor(super::agents::lower_supervisor(
            ctx,
            SupervisorDecl::cast(node)?,
        )),
        SyntaxKind::USE_DECL => Item::Use(lower_use(UseDecl::cast(node)?)),
        SyntaxKind::MOD_DECL => Item::Mod(lower_mod(ModDecl::cast(node)?)),
        // EXTERN_BLOCK, EXPORT_DECL, MACRO_DECL, IMPL_BLOCK, TRAIT_DECL, CONST_DECL — Task 22+
        _ => return None,
    };
    Some(ctx.package.items.alloc(item))
}

fn lower_fn(ctx: &mut LoweringCtx, f: FnDecl) -> FnId {
    let name = f.name().map(|n| n.text()).unwrap_or_default();
    let is_pub = f.is_pub();
    let is_unsafe = f.is_unsafe();
    let params = f
        .param_list()
        .map(|pl| {
            pl.0.children()
                .filter_map(sdust_ast::FnParam::cast)
                .map(|p| {
                    let pname =
                        p.0.children()
                            .find_map(sdust_ast::Name::cast)
                            .map(|n| n.text())
                            .unwrap_or_default();
                    let ty =
                        p.0.children()
                            .find(|c| is_type_node(c.kind()))
                            .map(|n| super::types::lower_type(ctx, n));
                    HirParam {
                        name: pname,
                        ty,
                        span: span_of(&p.0),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let ret = f
        .ret_type()
        .and_then(|r| r.0.children().next())
        .map(|t| super::types::lower_type(ctx, t));
    let effects = f
        .effect_clause()
        .map(|e| {
            e.0.children()
                .filter_map(sdust_ast::Name::cast)
                .map(|n| n.text())
                .collect()
        })
        .unwrap_or_default();
    let body = f.body().map(|b| super::exprs::lower_block(ctx, b));
    let generics = lower_generics(&f.0);
    let hf = HirFn {
        name,
        is_pub,
        is_unsafe,
        generics,
        params,
        ret,
        effects,
        body,
        span: span_of(&f.0),
    };
    ctx.package.fns.alloc(hf)
}

fn lower_struct(ctx: &mut LoweringCtx, s: StructDecl) -> StructId {
    let name =
        s.0.children()
            .find_map(sdust_ast::Name::cast)
            .map(|n| n.text())
            .unwrap_or_default();
    let fields =
        s.0.descendants()
            .filter_map(sdust_ast::StructField::cast)
            .map(|f| {
                let fname =
                    f.0.children()
                        .find_map(sdust_ast::Name::cast)
                        .map(|n| n.text())
                        .unwrap_or_default();
                let ty =
                    f.0.children()
                        .find(|c| is_type_node(c.kind()))
                        .map(|n| super::types::lower_type(ctx, n))
                        .unwrap_or_else(|| ctx.alloc_type(HirType::Unknown));
                HirStructField {
                    name: fname,
                    ty,
                    span: span_of(&f.0),
                }
            })
            .collect();
    let generics = lower_generics(&s.0);
    let hs = HirStruct {
        name,
        is_pub: has_visibility(&s.0),
        generics,
        fields,
        span: span_of(&s.0),
    };
    ctx.package.structs.alloc(hs)
}

fn lower_enum(ctx: &mut LoweringCtx, e: EnumDecl) -> EnumId {
    let name =
        e.0.children()
            .find_map(sdust_ast::Name::cast)
            .map(|n| n.text())
            .unwrap_or_default();
    let variants =
        e.0.descendants()
            .filter_map(sdust_ast::EnumVariant::cast)
            .map(|v| {
                let vname =
                    v.0.children()
                        .find_map(sdust_ast::Name::cast)
                        .map(|n| n.text())
                        .unwrap_or_default();
                let payload =
                    v.0.children()
                        .filter(|c| is_type_node(c.kind()))
                        .map(|n| super::types::lower_type(ctx, n))
                        .collect();
                HirEnumVariant {
                    name: vname,
                    payload,
                    span: span_of(&v.0),
                }
            })
            .collect();
    let generics = lower_generics(&e.0);
    let he = HirEnum {
        name,
        is_pub: has_visibility(&e.0),
        generics,
        variants,
        span: span_of(&e.0),
    };
    ctx.package.enums.alloc(he)
}

fn lower_type_alias(ctx: &mut LoweringCtx, t: TypeAlias) -> TypeAliasId {
    let name =
        t.0.children()
            .find_map(sdust_ast::Name::cast)
            .map(|n| n.text())
            .unwrap_or_default();
    let ty =
        t.0.children()
            .find(|c| is_type_node(c.kind()))
            .map(|n| super::types::lower_type(ctx, n))
            .unwrap_or_else(|| ctx.alloc_type(HirType::Unknown));
    let generics = lower_generics(&t.0);
    let h = HirTypeAlias {
        name,
        is_pub: has_visibility(&t.0),
        generics,
        ty,
        span: span_of(&t.0),
    };
    ctx.package.type_aliases.alloc(h)
}

fn lower_use(u: UseDecl) -> HirUse {
    let path: Vec<String> =
        u.0.descendants()
            .filter_map(sdust_ast::NameRef::cast)
            .map(|n| {
                n.0.first_token()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default()
            })
            .collect();
    HirUse {
        path,
        alias: None,
        leaves: vec![],
        span: span_of(&u.0),
    }
}

fn lower_mod(m: ModDecl) -> HirMod {
    let path: Vec<String> =
        m.0.descendants()
            .filter_map(sdust_ast::NameRef::cast)
            .map(|n| {
                n.0.first_token()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default()
            })
            .collect();
    HirMod {
        path,
        span: span_of(&m.0),
    }
}

pub fn is_type_node(k: SyntaxKind) -> bool {
    matches!(
        k,
        SyntaxKind::TYPE_PATH
            | SyntaxKind::TYPE_BORROW
            | SyntaxKind::TYPE_TUPLE
            | SyntaxKind::TYPE_ARRAY
            | SyntaxKind::TYPE_FN
            | SyntaxKind::TYPE_RESULT_SUGAR
            | SyntaxKind::TYPE_UNION
            | SyntaxKind::TYPE_DYN
    )
}

pub fn has_visibility(n: &SyntaxNode) -> bool {
    n.children().any(|c| c.kind() == SyntaxKind::VISIBILITY)
}

/// Collect generic parameter names from a `GENERIC_PARAM_LIST` child of `n`.
/// Returns an empty vec if the node has no generic params.
pub fn lower_generics(n: &SyntaxNode) -> Vec<String> {
    let Some(list) = n
        .children()
        .find(|c| c.kind() == SyntaxKind::GENERIC_PARAM_LIST)
    else {
        return vec![];
    };
    list.children()
        .filter(|c| c.kind() == SyntaxKind::GENERIC_PARAM)
        .map(|p| {
            p.children()
                .find_map(sdust_ast::Name::cast)
                .map(|n| n.text())
                .unwrap_or_default()
        })
        .collect()
}
