use crate::nodes::*;
use crate::ids::*;
use sdust_ast::AstNode;
use sdust_syntax::{SyntaxNode, SyntaxKind};
use super::LoweringCtx;

pub fn lower_pat(ctx: &mut LoweringCtx, n: SyntaxNode) -> PatId {
    let p = match n.kind() {
        SyntaxKind::WILDCARD_PAT => HirPat::Wildcard,
        SyntaxKind::LITERAL_PAT => {
            let tok = n.first_token().unwrap();
            HirPat::Literal(super::exprs::lower_literal_token(&tok))
        }
        SyntaxKind::BINDING_PAT => {
            let name = n.children().find_map(sdust_ast::Name::cast).map(|nm| nm.text()).unwrap_or_default();
            let sub = n.children().find(|c| is_pat_node(c.kind())).map(|sn| lower_pat(ctx, sn));
            HirPat::Binding { name, sub }
        }
        SyntaxKind::REF_PAT => {
            let mutable = n.children_with_tokens().filter_map(|e| e.into_token()).any(|t| t.kind() == SyntaxKind::MUT_KW);
            let inner = n.children().find(|c| is_pat_node(c.kind())).map(|p| lower_pat(ctx, p))
                .unwrap_or_else(|| ctx.alloc_pat(HirPat::Wildcard));
            HirPat::Ref { mutable, inner }
        }
        SyntaxKind::TUPLE_PAT => HirPat::Tuple(
            n.children().filter(|c| is_pat_node(c.kind())).map(|p| lower_pat(ctx, p)).collect()
        ),
        SyntaxKind::STRUCT_PAT => {
            let path = path_segments(&n);
            let fields = n.descendants().filter(|c| c.kind() == SyntaxKind::IDENT_PAT || c.kind() == SyntaxKind::BINDING_PAT)
                .map(|f| {
                    let nm = f.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default();
                    let sub = f.children().find(|c| is_pat_node(c.kind())).map(|p| lower_pat(ctx, p));
                    (nm, sub)
                }).collect();
            HirPat::Struct { path, fields }
        }
        SyntaxKind::ENUM_PAT => {
            let path = path_segments(&n);
            let args = n.children().filter(|c| is_pat_node(c.kind())).map(|p| lower_pat(ctx, p)).collect();
            HirPat::Enum { path, args }
        }
        SyntaxKind::RANGE_PAT => {
            let mut sub = n.children().filter(|c| is_pat_node(c.kind()));
            let lo = sub.next().map(|p| lower_pat(ctx, p)).unwrap_or_else(|| ctx.alloc_pat(HirPat::Wildcard));
            let hi = sub.next().map(|p| lower_pat(ctx, p)).unwrap_or_else(|| ctx.alloc_pat(HirPat::Wildcard));
            let inclusive = n.children_with_tokens().filter_map(|e| e.into_token()).any(|t| t.kind() == SyntaxKind::DOT_DOT_EQ);
            HirPat::Range { lo, hi, inclusive }
        }
        _ => HirPat::Wildcard,
    };
    ctx.alloc_pat(p)
}

pub fn is_pat_node(k: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(k, LITERAL_PAT | IDENT_PAT | WILDCARD_PAT | TUPLE_PAT
        | STRUCT_PAT | ENUM_PAT | RANGE_PAT | BINDING_PAT | REF_PAT)
}

fn path_segments(n: &SyntaxNode) -> Vec<String> {
    n.descendants().filter_map(sdust_ast::NameRef::cast)
        .map(|nr| nr.0.first_token().map(|t| t.text().to_string()).unwrap_or_default()).collect()
}
