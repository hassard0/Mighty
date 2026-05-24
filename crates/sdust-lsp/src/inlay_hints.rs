//! Inlay hints — `textDocument/inlayHint`.
//!
//! Emits `: I32`-style annotations next to `let` bindings whose type
//! the compiler inferred (i.e. the user did not write `: T`). The hint
//! type is read from the type checker's `expr_ty` table by walking the
//! fn's HIR body's `Let` statements in declaration order and pairing
//! them with the CST `LET_STMT` nodes inside the same fn body.
//!
//! Coverage in v0.5:
//! - `let x = expr` (no type annotation) — inferred-type hint.
//! - `fn f(p)` parameters without a written type — parameter-type hint
//!   (only if the parameter has no `: T` in the CST and the checker
//!   inferred a non-`{integer}` type).
//!
//! Deferred:
//! - Closure parameter hints (CST tracks them under `LAMBDA_EXPR`; we
//!   don't yet pair them with HIR closure params).
//! - Argument-name hints (would need per-call-site overload resolution).
//!
//! The handler honors the `range` parameter (a viewport) by emitting
//! only hints whose position falls inside it; the editor expects this.

use crate::docs::DocAnalysis;
use sdust_hir::{HirFn, HirStmt, Item, Package};
use sdust_syntax::{SyntaxKind, SyntaxNode};
use sdust_types::{pretty_ty, TyArena, TyData, TyId};
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position, Range};

/// Top-level entry — used by the LSP handler.
pub fn inlay_hints(doc: &DocAnalysis, viewport: Range) -> Vec<InlayHint> {
    let mut out: Vec<InlayHint> = Vec::new();
    let root = SyntaxNode::new_root(doc.parsed.green.clone());

    // For each top-level fn item in the HIR, find its CST FN_DECL by
    // name and walk the LET_STMT children of its body in declaration
    // order. Pair them with HirStmt::Let in the same order.
    for fn_id in iter_top_level_fns(&doc.package) {
        let f = &doc.package.fns[fn_id];
        if f.body.is_none() {
            continue;
        }
        let Some(fn_cst) = find_fn_decl(&root, &f.name) else {
            continue;
        };
        emit_param_hints(doc, &fn_cst, f, &mut out);

        let block_id = f.body.unwrap();
        let block = &doc.package.blocks[block_id];
        let hir_lets: Vec<&HirStmt> = block
            .stmts
            .iter()
            .filter(|s| matches!(s, HirStmt::Let { .. }))
            .collect();

        let cst_block = find_fn_body_block(&fn_cst);
        let cst_lets: Vec<SyntaxNode> = cst_block
            .map(|b| {
                b.children()
                    .filter(|c| c.kind() == SyntaxKind::LET_STMT)
                    .collect()
            })
            .unwrap_or_default();

        for (hir, cst) in hir_lets.iter().zip(cst_lets.iter()) {
            let HirStmt::Let { ty, init, .. } = hir else {
                continue;
            };
            if ty.is_some() {
                // user wrote a type annotation — no hint needed
                continue;
            }
            let Some(init_eid) = init else {
                continue;
            };
            // No annotation in CST either?
            if let_has_type_annotation(cst) {
                continue;
            }
            let Some(tyid) = doc.typed.expr_ty.get(init_eid).copied() else {
                continue;
            };
            // Skip uninteresting unresolved fresh vars.
            if is_uninteresting_ty(tyid, &doc.typed.ty_arena) {
                continue;
            }
            let Some(insert_after) = let_binding_end(cst) else {
                continue;
            };
            let (line, character) = doc.line_index.offset_to_position(&doc.source, insert_after);
            let pos = Position { line, character };
            if !pos_in_range(pos, viewport) {
                continue;
            }
            let rendered = pretty_ty(tyid, &doc.typed.ty_arena, None, Some(&doc.typed.def_map));
            out.push(InlayHint {
                position: pos,
                label: InlayHintLabel::String(format!(": {}", rendered)),
                kind: Some(InlayHintKind::TYPE),
                text_edits: None,
                tooltip: None,
                padding_left: Some(false),
                padding_right: Some(true),
                data: None,
            });
        }
    }
    out
}

fn emit_param_hints(doc: &DocAnalysis, fn_cst: &SyntaxNode, f: &HirFn, out: &mut Vec<InlayHint>) {
    // Walk FN_PARAM children, find each IDENT binding without a `:`
    // type annotation. Look up the matching HirParam by name+order to
    // get the inferred type via fn_params side table.
    let Some(plist) = fn_cst
        .children()
        .find(|c| c.kind() == SyntaxKind::FN_PARAM_LIST)
    else {
        return;
    };
    let Some(fn_id) =
        doc.package
            .fns
            .iter()
            .find_map(|(id, fdef)| if fdef.name == f.name { Some(id) } else { None })
    else {
        return;
    };
    let typed_params = match doc.typed.fn_params.get(&fn_id) {
        Some(ps) => ps,
        None => return,
    };
    for (i, param_cst) in plist
        .children()
        .filter(|c| c.kind() == SyntaxKind::FN_PARAM)
        .enumerate()
    {
        if param_has_type_annotation(&param_cst) {
            continue;
        }
        let Some((_, tyid)) = typed_params.get(i) else {
            continue;
        };
        if is_uninteresting_ty(*tyid, &doc.typed.ty_arena) {
            continue;
        }
        let Some(ident) = param_cst
            .children_with_tokens()
            .filter_map(|c| c.into_token())
            .find(|t| t.kind() == SyntaxKind::IDENT)
        else {
            continue;
        };
        let end: u32 = ident.text_range().end().into();
        let (line, character) = doc.line_index.offset_to_position(&doc.source, end);
        let rendered = pretty_ty(*tyid, &doc.typed.ty_arena, None, Some(&doc.typed.def_map));
        out.push(InlayHint {
            position: Position { line, character },
            label: InlayHintLabel::String(format!(": {}", rendered)),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: Some(false),
            padding_right: Some(true),
            data: None,
        });
    }
}

fn pos_in_range(p: Position, r: Range) -> bool {
    let after = (p.line, p.character) >= (r.start.line, r.start.character);
    let before = (p.line, p.character) <= (r.end.line, r.end.character);
    after && before
}

fn iter_top_level_fns(pkg: &Package) -> impl Iterator<Item = sdust_hir::FnId> + '_ {
    pkg.top_level.iter().filter_map(move |iid| {
        let item = &pkg.items[*iid];
        match item {
            Item::Fn(id) => Some(*id),
            _ => None,
        }
    })
}

fn find_fn_decl(root: &SyntaxNode, name: &str) -> Option<SyntaxNode> {
    root.descendants().find(|n| {
        n.kind() == SyntaxKind::FN_DECL
            && n.children()
                .find(|c| c.kind() == SyntaxKind::NAME)
                .map(|name_node| name_node.text() == name)
                .unwrap_or(false)
    })
}

fn find_fn_body_block(fn_cst: &SyntaxNode) -> Option<SyntaxNode> {
    fn_cst.children().find(|c| c.kind() == SyntaxKind::BLOCK)
}

fn let_has_type_annotation(let_stmt: &SyntaxNode) -> bool {
    // The CST shape is `let <pat> [: <type>] [= <expr>] ;`. A type
    // annotation shows up as a child TYPE_REF / TYPE_PATH etc.
    let_stmt.children().any(|c| is_type_node(c.kind()))
}

fn param_has_type_annotation(param_cst: &SyntaxNode) -> bool {
    param_cst.children().any(|c| is_type_node(c.kind()))
}

fn is_type_node(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::TYPE_REF
            | SyntaxKind::TYPE_PATH
            | SyntaxKind::TYPE_BORROW
            | SyntaxKind::TYPE_TUPLE
            | SyntaxKind::TYPE_ARRAY
            | SyntaxKind::TYPE_FN
            | SyntaxKind::TYPE_DYN
            | SyntaxKind::TYPE_RESULT_SUGAR
            | SyntaxKind::TYPE_UNION
    )
}

fn let_binding_end(let_stmt: &SyntaxNode) -> Option<u32> {
    // Position the hint after the binding's last IDENT character. For a
    // pattern like `(a, b)` we just put the hint after the closing
    // paren — works for `IDENT_PAT` directly and degrades cleanly for
    // tuple/struct patterns.
    let pat = let_stmt.children().find(|c| is_pattern_node(c.kind()))?;
    Some(pat.text_range().end().into())
}

fn is_pattern_node(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IDENT_PAT
            | SyntaxKind::WILDCARD_PAT
            | SyntaxKind::TUPLE_PAT
            | SyntaxKind::STRUCT_PAT
            | SyntaxKind::ENUM_PAT
            | SyntaxKind::BINDING_PAT
            | SyntaxKind::REF_PAT
            | SyntaxKind::LITERAL_PAT
            | SyntaxKind::RANGE_PAT
    )
}

fn is_uninteresting_ty(ty: TyId, arena: &TyArena) -> bool {
    matches!(
        arena.get(ty),
        TyData::Error | TyData::Var(_) | TyData::Param(_)
    )
}
