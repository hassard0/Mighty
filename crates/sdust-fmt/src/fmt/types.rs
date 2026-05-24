//! Canonical printers for type-level CST nodes.
//!
//! Each function takes a `SyntaxNode` of the corresponding kind and
//! returns a [`Doc`]. Nodes we haven't yet canonicalized fall back to
//! verbatim via [`super::verbatim`].
//!
//! These printers are exposed as library surface for future slices
//! (and for direct unit testing via `tests/canonical.rs`); the
//! slice-2 `file` printer still emits items verbatim, so they don't
//! drive end-to-end formatting yet.

use crate::doc::Doc;
use sdust_syntax::{SyntaxKind, SyntaxNode};

/// Format any type-level node.
pub fn type_expr(n: &SyntaxNode) -> Doc {
    match n.kind() {
        SyntaxKind::TYPE_PATH => type_path(n),
        SyntaxKind::TYPE_BORROW => type_borrow(n),
        SyntaxKind::TYPE_TUPLE => type_tuple(n),
        SyntaxKind::TYPE_ARRAY => type_array(n),
        SyntaxKind::TYPE_FN => type_fn(n),
        SyntaxKind::TYPE_RESULT_SUGAR => type_result_sugar(n),
        SyntaxKind::TYPE_UNION => type_union(n),
        SyntaxKind::PATH => path_node(n),
        _ => super::verbatim(n),
    }
}

fn type_path(n: &SyntaxNode) -> Doc {
    let mut parts = Vec::new();
    for child in n.children() {
        match child.kind() {
            SyntaxKind::PATH => parts.push(path_node(&child)),
            SyntaxKind::GENERIC_ARG_LIST => parts.push(generic_args(&child)),
            _ => {}
        }
    }
    Doc::concat_all(parts)
}

/// Render a PATH node: PATH_SEGMENT children joined with `.`. Each
/// segment may carry a `::[T,...]` turbofish suffix.
pub fn path_node(n: &SyntaxNode) -> Doc {
    let mut segs = Vec::new();
    for seg in n
        .children()
        .filter(|c| c.kind() == SyntaxKind::PATH_SEGMENT)
    {
        let name = seg
            .children()
            .find(|c| c.kind() == SyntaxKind::NAME_REF)
            .map(|nr| Doc::text(nr.text().to_string()))
            .unwrap_or(Doc::nil());
        let turbofish = seg
            .children()
            .find(|c| c.kind() == SyntaxKind::GENERIC_ARG_LIST)
            .map(|gl| Doc::concat(Doc::text("::"), generic_args(&gl)));
        let mut seg_doc = name;
        if let Some(t) = turbofish {
            seg_doc = Doc::concat(seg_doc, t);
        }
        segs.push(seg_doc);
    }
    Doc::join(Doc::text("."), segs)
}

fn generic_args(n: &SyntaxNode) -> Doc {
    let args: Vec<Doc> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::GENERIC_ARG)
        .filter_map(|g| g.children().next().map(|t| type_expr(&t)))
        .collect();
    Doc::concat(
        Doc::text("["),
        Doc::concat(Doc::join(Doc::text(", "), args), Doc::text("]")),
    )
}

fn type_borrow(n: &SyntaxNode) -> Doc {
    let has_mut = n
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::MUT_KW);
    let head = if has_mut {
        Doc::text("&mut ")
    } else {
        Doc::text("&")
    };
    let inner = n
        .children()
        .next()
        .map(|c| type_expr(&c))
        .unwrap_or(Doc::nil());
    Doc::concat(head, inner)
}

fn type_tuple(n: &SyntaxNode) -> Doc {
    let parts: Vec<Doc> = n.children().map(|c| type_expr(&c)).collect();
    Doc::concat(
        Doc::text("("),
        Doc::concat(Doc::join(Doc::text(", "), parts), Doc::text(")")),
    )
}

fn type_array(n: &SyntaxNode) -> Doc {
    let mut children = n.children();
    let elem = children.next().map(|c| type_expr(&c)).unwrap_or(Doc::nil());
    let len = children.next().map(|c| super::verbatim(&c));
    let inner = match len {
        Some(l) => Doc::concat(elem, Doc::concat(Doc::text("; "), l)),
        None => elem,
    };
    Doc::concat(Doc::text("["), Doc::concat(inner, Doc::text("]")))
}

fn type_fn(n: &SyntaxNode) -> Doc {
    let all_children: Vec<SyntaxNode> = n.children().collect();
    let has_ret = n
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::THIN_ARROW);
    let (params, ret) = if has_ret && !all_children.is_empty() {
        let mut p: Vec<&SyntaxNode> = all_children.iter().collect();
        let r = p.pop();
        (p, r)
    } else {
        (all_children.iter().collect::<Vec<_>>(), None)
    };
    let param_docs: Vec<Doc> = params.into_iter().map(type_expr).collect();
    let body = Doc::concat(
        Doc::text("fn("),
        Doc::concat(Doc::join(Doc::text(", "), param_docs), Doc::text(")")),
    );
    match ret {
        Some(r) => Doc::concat(body, Doc::concat(Doc::text(" -> "), type_expr(r))),
        None => body,
    }
}

fn type_result_sugar(n: &SyntaxNode) -> Doc {
    let mut children = n.children();
    let ok = children.next().map(|c| type_expr(&c)).unwrap_or(Doc::nil());
    let err = children.next().map(|c| type_expr(&c)).unwrap_or(Doc::nil());
    Doc::concat(ok, Doc::concat(Doc::text("!"), err))
}

fn type_union(n: &SyntaxNode) -> Doc {
    let parts: Vec<Doc> = n.children().map(|c| type_expr(&c)).collect();
    Doc::concat(
        Doc::text("{"),
        Doc::concat(Doc::join(Doc::text(", "), parts), Doc::text("}")),
    )
}
