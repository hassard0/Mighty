//! Canonical printers for expression CST nodes.
//!
//! Exposed as library surface; the slice-2 `file` printer emits items
//! verbatim. Block-bearing constructs (if/match/for/while/loop/blocks)
//! and the agent/protocol/supervisor families still fall back to
//! verbatim — those land in a later slice once we're confident the
//! restructuring stays round-trip-safe.

use crate::doc::Doc;
use mty_syntax::{SyntaxKind, SyntaxNode};

pub fn expr(n: &SyntaxNode) -> Doc {
    match n.kind() {
        SyntaxKind::LITERAL_EXPR => Doc::text(n.text().to_string()),
        SyntaxKind::PATH_EXPR => path_expr(n),
        SyntaxKind::BINARY_EXPR => binary_expr(n),
        SyntaxKind::UNARY_EXPR => unary_expr(n),
        SyntaxKind::CALL_EXPR => call_expr(n),
        SyntaxKind::METHOD_CALL_EXPR => method_call_expr(n),
        SyntaxKind::FIELD_EXPR => field_expr(n),
        SyntaxKind::INDEX_EXPR => index_expr(n),
        SyntaxKind::TUPLE_EXPR => tuple_or_paren(n),
        SyntaxKind::ARRAY_EXPR => array_expr(n),
        SyntaxKind::SEND_EXPR => send_or_ask(n, "!"),
        SyntaxKind::ASK_EXPR => send_or_ask(n, "?"),
        SyntaxKind::QUESTION_EXPR => Doc::concat(first_child_expr(n), Doc::text("?")),
        SyntaxKind::DEADLINE_EXPR => deadline_expr(n),
        SyntaxKind::RUN_EXPR => run_expr(n),
        _ => super::verbatim(n),
    }
}

fn path_expr(n: &SyntaxNode) -> Doc {
    n.children()
        .find(|c| c.kind() == SyntaxKind::PATH)
        .map(|p| super::types::path_node(&p))
        .unwrap_or_else(|| super::verbatim(n))
}

fn binary_expr(n: &SyntaxNode) -> Doc {
    let kids: Vec<SyntaxNode> = n.children().collect();
    if kids.len() != 2 {
        return super::verbatim(n);
    }
    let op = n
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| !t.kind().is_trivia())
        .map(|t| t.text().to_string())
        .unwrap_or_default();
    Doc::concat(
        expr(&kids[0]),
        Doc::concat(Doc::text(format!(" {} ", op)), expr(&kids[1])),
    )
}

fn unary_expr(n: &SyntaxNode) -> Doc {
    let op = n
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| !t.kind().is_trivia())
        .map(|t| t.text().to_string())
        .unwrap_or_default();
    let inner = first_child_expr(n);
    Doc::concat(Doc::text(op), inner)
}

fn call_expr(n: &SyntaxNode) -> Doc {
    let callee = first_child_expr(n);
    let args = n
        .children()
        .find(|c| c.kind() == SyntaxKind::ARG_LIST)
        .map(arg_list)
        .unwrap_or(Doc::text("()"));
    Doc::concat(callee, args)
}

fn method_call_expr(n: &SyntaxNode) -> Doc {
    let receiver = first_child_expr(n);
    let name = n
        .children()
        .find(|c| c.kind() == SyntaxKind::NAME)
        .map(|nm| Doc::text(nm.text().to_string()))
        .unwrap_or(Doc::nil());
    let args = n
        .children()
        .find(|c| c.kind() == SyntaxKind::ARG_LIST)
        .map(arg_list)
        .unwrap_or(Doc::text("()"));
    Doc::concat(
        receiver,
        Doc::concat(Doc::text("."), Doc::concat(name, args)),
    )
}

fn field_expr(n: &SyntaxNode) -> Doc {
    let receiver = first_child_expr(n);
    let name = n
        .children()
        .find(|c| c.kind() == SyntaxKind::NAME)
        .map(|nm| Doc::text(nm.text().to_string()))
        .unwrap_or(Doc::nil());
    Doc::concat(receiver, Doc::concat(Doc::text("."), name))
}

fn index_expr(n: &SyntaxNode) -> Doc {
    let kids: Vec<SyntaxNode> = n.children().collect();
    let recv = kids.first().map(expr).unwrap_or(Doc::nil());
    let idx = kids.get(1).map(expr).unwrap_or(Doc::nil());
    Doc::concat(
        recv,
        Doc::concat(Doc::text("["), Doc::concat(idx, Doc::text("]"))),
    )
}

fn tuple_or_paren(n: &SyntaxNode) -> Doc {
    let parts: Vec<Doc> = n.children().map(|c| expr(&c)).collect();
    Doc::concat(
        Doc::text("("),
        Doc::concat(Doc::join(Doc::text(", "), parts), Doc::text(")")),
    )
}

fn array_expr(n: &SyntaxNode) -> Doc {
    let parts: Vec<Doc> = n.children().map(|c| expr(&c)).collect();
    Doc::concat(
        Doc::text("["),
        Doc::concat(Doc::join(Doc::text(", "), parts), Doc::text("]")),
    )
}

fn send_or_ask(n: &SyntaxNode, sigil: &str) -> Doc {
    let target = first_child_expr(n);
    let msg = n
        .children()
        .find(|c| c.kind() == SyntaxKind::NAME)
        .map(|nm| Doc::text(nm.text().to_string()))
        .unwrap_or(Doc::nil());
    let args = n
        .children()
        .find(|c| c.kind() == SyntaxKind::ARG_LIST)
        .map(arg_list)
        .unwrap_or(Doc::nil());
    Doc::concat(
        target,
        Doc::concat(Doc::text(sigil.to_string()), Doc::concat(msg, args)),
    )
}

fn deadline_expr(n: &SyntaxNode) -> Doc {
    let kids: Vec<SyntaxNode> = n.children().collect();
    let inner = kids.first().map(expr).unwrap_or(Doc::nil());
    let dur = kids.get(1).map(expr).unwrap_or(Doc::nil());
    Doc::concat(inner, Doc::concat(Doc::text(" @"), dur))
}

fn run_expr(n: &SyntaxNode) -> Doc {
    let inner = first_child_expr(n);
    Doc::concat(Doc::text("run "), inner)
}

fn first_child_expr(n: &SyntaxNode) -> Doc {
    n.children().next().map(|c| expr(&c)).unwrap_or(Doc::nil())
}

fn arg_list(n: SyntaxNode) -> Doc {
    let parts: Vec<Doc> = n
        .children()
        .map(|c| match c.kind() {
            SyntaxKind::NAMED_ARG => named_arg(&c),
            SyntaxKind::ARG => c.children().next().map(|e| expr(&e)).unwrap_or(Doc::nil()),
            _ => super::verbatim(&c),
        })
        .collect();
    Doc::concat(
        Doc::text("("),
        Doc::concat(Doc::join(Doc::text(", "), parts), Doc::text(")")),
    )
}

fn named_arg(n: &SyntaxNode) -> Doc {
    let name = n
        .children()
        .find(|c| c.kind() == SyntaxKind::NAME)
        .map(|nm| Doc::text(nm.text().to_string()))
        .unwrap_or(Doc::nil());
    let val = n
        .children()
        .find(|c| c.kind() != SyntaxKind::NAME)
        .map(|e| expr(&e))
        .unwrap_or(Doc::nil());
    Doc::concat(name, Doc::concat(Doc::text(": "), val))
}
