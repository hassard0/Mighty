use super::LoweringCtx;
use crate::ids::*;
use crate::nodes::*;
use mty_ast::AstNode;
use mty_syntax::{SyntaxKind, SyntaxNode};

pub fn lower_type(ctx: &mut LoweringCtx, n: SyntaxNode) -> TypeId {
    let t = match n.kind() {
        SyntaxKind::TYPE_PATH => {
            // Collect segments only from the immediate PATH child (not
            // through descendants — that would pick up names inside
            // GENERIC_ARG_LIST too, e.g. `Option[I32, Str]` would yield
            // segments=["Option","I32","Str"]).
            let segs: Vec<String> = n
                .children()
                .filter(|c| c.kind() == SyntaxKind::PATH)
                .flat_map(|p| p.descendants())
                .filter_map(mty_ast::NameRef::cast)
                .map(|nr| {
                    nr.0.first_token()
                        .map(|t| t.text().to_string())
                        .unwrap_or_default()
                })
                .collect();
            let generics: Vec<TypeId> = n
                .children()
                .filter(|c| c.kind() == SyntaxKind::GENERIC_ARG_LIST)
                .flat_map(|gl| gl.children())
                .filter(|c| c.kind() == SyntaxKind::GENERIC_ARG)
                .flat_map(|ga| ga.children())
                .filter(|c| super::items::is_type_node(c.kind()))
                .map(|tn| lower_type(ctx, tn))
                .collect();
            HirType::Path {
                segments: segs,
                generics,
            }
        }
        SyntaxKind::TYPE_BORROW => {
            let mutable = n
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .any(|t| t.kind() == SyntaxKind::MUT_KW);
            let inner = n
                .children()
                .find(|c| super::items::is_type_node(c.kind()))
                .map(|tn| lower_type(ctx, tn))
                .unwrap_or_else(|| ctx.alloc_type(HirType::Unknown));
            HirType::Borrow { mutable, inner }
        }
        SyntaxKind::TYPE_TUPLE => {
            let elems: Vec<_> = n
                .children()
                .filter(|c| super::items::is_type_node(c.kind()))
                .map(|tn| lower_type(ctx, tn))
                .collect();
            if elems.is_empty() {
                HirType::Unit
            } else {
                HirType::Tuple(elems)
            }
        }
        SyntaxKind::TYPE_ARRAY => {
            let mut tys = n
                .children()
                .filter(|c| super::items::is_type_node(c.kind()));
            let elem = tys
                .next()
                .map(|tn| lower_type(ctx, tn))
                .unwrap_or_else(|| ctx.alloc_type(HirType::Unknown));
            HirType::Array { elem, len: None }
        }
        SyntaxKind::TYPE_FN => {
            let mut tys: Vec<_> = n
                .children()
                .filter(|c| super::items::is_type_node(c.kind()))
                .collect();
            let ret = tys.pop().map(|tn| lower_type(ctx, tn));
            let params: Vec<_> = tys.into_iter().map(|tn| lower_type(ctx, tn)).collect();
            HirType::Fn { params, ret }
        }
        SyntaxKind::TYPE_RESULT_SUGAR => {
            let mut iter = n
                .children()
                .filter(|c| super::items::is_type_node(c.kind()));
            let ok = iter
                .next()
                .map(|tn| lower_type(ctx, tn))
                .unwrap_or_else(|| ctx.alloc_type(HirType::Unknown));
            let err = iter
                .next()
                .map(|tn| lower_type(ctx, tn))
                .unwrap_or_else(|| ctx.alloc_type(HirType::Unknown));
            HirType::Result { ok, err }
        }
        SyntaxKind::TYPE_UNION => {
            let elems: Vec<_> = n
                .children()
                .filter(|c| super::items::is_type_node(c.kind()))
                .map(|tn| lower_type(ctx, tn))
                .collect();
            HirType::Union(elems)
        }
        SyntaxKind::TYPE_DYN => {
            // Single trait identifier; pick the first PATH descendant.
            let name = n
                .descendants()
                .find(|c| c.kind() == SyntaxKind::PATH)
                .and_then(|p| p.descendants().find_map(mty_ast::NameRef::cast))
                .and_then(|nr| nr.0.first_token().map(|t| t.text().to_string()))
                .unwrap_or_default();
            HirType::Dyn { trait_name: name }
        }
        _ => HirType::Unknown,
    };
    ctx.alloc_type(t)
}
