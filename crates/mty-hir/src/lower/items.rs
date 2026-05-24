use super::{span_of, LoweringCtx};
use crate::ids::*;
use crate::nodes::*;
use mty_ast::{
    AgentDecl, AstNode, EnumDecl, FnDecl, ModDecl, ProtocolDecl, StructDecl, SupervisorDecl,
    TypeAlias, UseDecl,
};
use mty_syntax::{SyntaxKind, SyntaxNode};

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
        SyntaxKind::EXTERN_BLOCK => Item::ExternBlock(lower_extern_block(ctx, node)),
        SyntaxKind::IMPL_BLOCK => Item::Impl(lower_impl_block(ctx, node)),
        SyntaxKind::TRAIT_DECL => Item::Trait(lower_trait_decl(ctx, node)),
        SyntaxKind::SANDBOX_BLOCK => Item::Sandbox(super::exprs::lower_top_sandbox(ctx, node)),
        // EXPORT_DECL, MACRO_DECL, CONST_DECL — later slices.
        _ => return None,
    };
    Some(ctx.package.items.alloc(item))
}

/// Same as `lower_fn` but with public visibility (for use from other
/// lowering modules — e.g. agent-method collection).
pub fn lower_fn_public(ctx: &mut LoweringCtx, f: FnDecl) -> FnId {
    lower_fn(ctx, f)
}

fn lower_fn(ctx: &mut LoweringCtx, f: FnDecl) -> FnId {
    let name = f.name().map(|n| n.text()).unwrap_or_default();
    let is_pub = f.is_pub();
    let is_unsafe = f.is_unsafe();
    let params = f
        .param_list()
        .map(|pl| {
            pl.0.children()
                .filter_map(mty_ast::FnParam::cast)
                .map(|p| {
                    let pname =
                        p.0.children()
                            .find_map(mty_ast::Name::cast)
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
                .filter_map(mty_ast::Name::cast)
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
            .find_map(mty_ast::Name::cast)
            .map(|n| n.text())
            .unwrap_or_default();
    let fields =
        s.0.descendants()
            .filter_map(mty_ast::StructField::cast)
            .map(|f| {
                let fname =
                    f.0.children()
                        .find_map(mty_ast::Name::cast)
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
    let derives = collect_derives(&s.0);
    let hs = HirStruct {
        name,
        is_pub: has_visibility(&s.0),
        generics,
        fields,
        derives,
        span: span_of(&s.0),
    };
    ctx.package.structs.alloc(hs)
}

fn lower_enum(ctx: &mut LoweringCtx, e: EnumDecl) -> EnumId {
    let name =
        e.0.children()
            .find_map(mty_ast::Name::cast)
            .map(|n| n.text())
            .unwrap_or_default();
    let variants =
        e.0.descendants()
            .filter_map(mty_ast::EnumVariant::cast)
            .map(|v| {
                let vname =
                    v.0.children()
                        .find_map(mty_ast::Name::cast)
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
    let derives = collect_derives(&e.0);
    let he = HirEnum {
        name,
        is_pub: has_visibility(&e.0),
        generics,
        variants,
        derives,
        span: span_of(&e.0),
    };
    ctx.package.enums.alloc(he)
}

/// Slice 5: extract derive names from `ATTR` children of an item. The
/// parser wraps `#[derive(...)]` inside the item's checkpoint, so ATTR
/// nodes are immediate children of STRUCT_DECL / ENUM_DECL.
pub fn collect_derives(item: &SyntaxNode) -> Vec<String> {
    use mty_syntax::SyntaxKind as SK;
    let mut out: Vec<String> = vec![];
    for attr in item.children().filter(|c| c.kind() == SK::ATTR) {
        let names: Vec<String> = attr
            .children()
            .filter_map(mty_ast::Name::cast)
            .map(|n| n.text())
            .collect();
        // Drop the leading "derive" sentinel when present.
        let derived = if names.first().map(|s| s.as_str()) == Some("derive") {
            names[1..].to_vec()
        } else {
            names
        };
        out.extend(derived);
    }
    out
}

fn lower_type_alias(ctx: &mut LoweringCtx, t: TypeAlias) -> TypeAliasId {
    let name =
        t.0.children()
            .find_map(mty_ast::Name::cast)
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
            .filter_map(mty_ast::NameRef::cast)
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
            .filter_map(mty_ast::NameRef::cast)
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

fn lower_extern_block(ctx: &mut LoweringCtx, node: SyntaxNode) -> HirExternBlock {
    let span = span_of(&node);
    // Optional ABI tag is the first NAME child (only if present).
    let abi = node
        .children()
        .find(|c| c.kind() == SyntaxKind::NAME)
        .and_then(|n| n.first_token())
        .map(|t| t.text().to_string());
    let mut fns: Vec<FnId> = vec![];
    for child in node.children() {
        if child.kind() != SyntaxKind::EXTERN_FN {
            continue;
        }
        // EXTERN_FN: parse like fn_decl but with no body.
        let name = child
            .children()
            .find_map(mty_ast::Name::cast)
            .map(|n| n.text())
            .unwrap_or_default();
        let params = child
            .children()
            .find(|c| c.kind() == SyntaxKind::FN_PARAM_LIST)
            .map(|pl| {
                pl.children()
                    .filter_map(mty_ast::FnParam::cast)
                    .map(|p| {
                        let pname =
                            p.0.children()
                                .find_map(mty_ast::Name::cast)
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
        let ret = child
            .children()
            .find(|c| c.kind() == SyntaxKind::RET_TYPE)
            .and_then(|r| r.children().next())
            .map(|t| super::types::lower_type(ctx, t));
        let hf = HirFn {
            name,
            is_pub: true,
            is_unsafe: false,
            generics: vec![],
            params,
            ret,
            effects: vec![],
            body: None,
            span: span_of(&child),
        };
        let fid = ctx.package.fns.alloc(hf);
        fns.push(fid);
    }
    HirExternBlock { abi, fns, span }
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

/// Lower an `impl [Trait for] T { ... }` block.
pub fn lower_impl_block(ctx: &mut LoweringCtx, node: SyntaxNode) -> HirImpl {
    let span = span_of(&node);
    // The first type child is either the trait (if `for` follows) or
    // the self type. Detect `for` by token presence in children_with_tokens.
    let type_children: Vec<SyntaxNode> =
        node.children().filter(|c| is_type_node(c.kind())).collect();
    let has_for = node
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::FOR_KW);
    let (trait_for, self_ty) = if has_for && type_children.len() >= 2 {
        let trait_ty = super::types::lower_type(ctx, type_children[0].clone());
        let self_t = super::types::lower_type(ctx, type_children[1].clone());
        (Some(trait_ty), self_t)
    } else if !type_children.is_empty() {
        let self_t = super::types::lower_type(ctx, type_children[0].clone());
        (None, self_t)
    } else {
        (None, ctx.alloc_type(HirType::Unknown))
    };
    // Methods: each FN_DECL child becomes a HirFn.
    let mut methods: Vec<FnId> = vec![];
    for fn_node in node.children().filter(|c| c.kind() == SyntaxKind::FN_DECL) {
        if let Some(fd) = mty_ast::FnDecl::cast(fn_node) {
            let fid = lower_fn_public(ctx, fd);
            methods.push(fid);
        }
    }
    HirImpl {
        trait_for,
        self_ty,
        methods,
        span,
    }
}

/// Lower a `trait Name { fn m(...); ... }` block.
pub fn lower_trait_decl(ctx: &mut LoweringCtx, node: SyntaxNode) -> HirTrait {
    let span = span_of(&node);
    let name = node
        .children()
        .find_map(mty_ast::Name::cast)
        .map(|n| n.text())
        .unwrap_or_default();
    let is_pub = has_visibility(&node);
    let generics = lower_generics(&node);
    // Each TRAIT_METHOD child contains a single FN_DECL.
    let mut methods: Vec<FnId> = vec![];
    for tm in node
        .children()
        .filter(|c| c.kind() == SyntaxKind::TRAIT_METHOD)
    {
        if let Some(fd) = tm.children().find_map(mty_ast::FnDecl::cast) {
            let fid = lower_fn_public(ctx, fd);
            methods.push(fid);
        }
    }
    HirTrait {
        name,
        is_pub,
        generics,
        methods,
        span,
    }
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
                .find_map(mty_ast::Name::cast)
                .map(|n| n.text())
                .unwrap_or_default()
        })
        .collect()
}
