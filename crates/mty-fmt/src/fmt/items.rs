//! File-level + item-level formatting.
//!
//! The file printer walks the FILE node's `children_with_tokens` and
//! emits a canonical whitespace layout:
//!
//! * Each top-level item's textual content is preserved verbatim unless
//!   the item has a dedicated canonical printer, with trailing
//!   whitespace stripped.
//! * Between two adjacent items, the separator is `\n\n` (one blank
//!   line) if the original source had a blank line between them, else
//!   `\n` (immediate succession).
//! * Comments that appear in file-level trivia between two items are
//!   preserved verbatim on their own lines, attached to the following
//!   item.
//! * Exactly one trailing `\n` at EOF.
//!
//! v0.43 added canonical printing for top-level `const` declarations.
//! v0.45 T2 extends the rollout to `fn` signatures, `struct`, `enum`,
//! and `type` aliases. The body of fn / struct / enum items is emitted
//! verbatim from source so we don't disturb statement-level formatting
//! or comments inside the block; only the *signature head* is rewritten.
//! Items whose signature carries comments or attribute prefixes still
//! fall back to verbatim, preserving the v0.43 safety pattern.

use crate::doc::Doc;
use mty_syntax::{SyntaxKind, SyntaxNode};

pub fn file(n: &SyntaxNode) -> Doc {
    let mut parts: Vec<Doc> = Vec::new();
    let mut first_emitted = false;
    // Trivia that lives **between** items at file-scope (rare — most
    // inter-item whitespace lives inside the items themselves, attached
    // by the parser as trailing whitespace).
    let mut pending_file_trivia = String::new();
    // Whether the previous item's stripped-off trailing whitespace
    // contained a blank line (i.e. `\n\n+`).
    let mut prev_blank_after = false;

    for child in n.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(t) => {
                if t.kind().is_trivia() {
                    pending_file_trivia.push_str(t.text());
                }
            }
            rowan::NodeOrToken::Node(item) => {
                let comments = extract_comment_lines(&pending_file_trivia);
                let file_blank = trivia_has_blank_line(&pending_file_trivia);

                if first_emitted {
                    let want_blank = prev_blank_after || file_blank || !comments.is_empty();
                    parts.push(Doc::text(if want_blank { "\n\n" } else { "\n" }));
                }

                if !comments.is_empty() {
                    parts.push(Doc::text(comments));
                    parts.push(Doc::text("\n"));
                }

                let (body, blank_after) = item_body_and_trailing_blank(&item);
                parts.push(body);

                first_emitted = true;
                pending_file_trivia.clear();
                prev_blank_after = blank_after;
            }
        }
    }

    // Trailing file-level trivia (anything after the last item). Comments
    // here are preserved on their own lines; whitespace is dropped because
    // `normalize_eof` enforces exactly one trailing newline.
    let tail_comments = extract_comment_lines(&pending_file_trivia);
    if !tail_comments.is_empty() {
        if first_emitted {
            parts.push(Doc::text("\n"));
        }
        parts.push(Doc::text(tail_comments));
    }

    // The `normalize_eof` step in `lib::format` ensures exactly one final
    // `\n`, so we don't need to push one here ourselves.

    Doc::concat_all(parts)
}

/// Returns the item's textual body with trailing whitespace stripped,
/// plus a flag indicating whether that trailing whitespace contained
/// a blank line (which is the cue to use a blank-line separator
/// before the next item).
fn item_body_and_trailing_blank(item: &SyntaxNode) -> (Doc, bool) {
    let text = item.text().to_string();
    let stripped = text.trim_end_matches(|c: char| c.is_whitespace());
    let tail = &text[stripped.len()..];
    let blank_after = tail.matches('\n').count() >= 2;
    let body = match item.kind() {
        SyntaxKind::CONST_DECL if can_canonicalize_const(item, stripped) => const_decl(item),
        SyntaxKind::FN_DECL => fn_decl_or_verbatim(item, stripped),
        SyntaxKind::STRUCT_DECL => struct_decl_or_verbatim(item, stripped),
        SyntaxKind::ENUM_DECL => enum_decl_or_verbatim(item, stripped),
        SyntaxKind::TYPE_ALIAS => type_alias_or_verbatim(item, stripped),
        _ => Doc::text(stripped.to_string()),
    };
    (body, blank_after)
}

fn can_canonicalize_const(item: &SyntaxNode, stripped: &str) -> bool {
    // Some parser paths attach same-line or following comments to the
    // declaration node. Keep those declarations verbatim until the item
    // printer can preserve attached trivia explicitly.
    item.kind() == SyntaxKind::CONST_DECL && !stripped.contains("//") && !stripped.contains("/*")
}

fn const_decl(item: &SyntaxNode) -> Doc {
    let is_pub = item.children().any(|c| c.kind() == SyntaxKind::VISIBILITY);
    let name = item
        .children()
        .find(|c| c.kind() == SyntaxKind::NAME)
        .map(|n| Doc::text(n.text().to_string()))
        .unwrap_or(Doc::nil());
    let ty = item
        .children()
        .find(|c| is_type_node(c.kind()))
        .map(|n| super::types::type_expr(&n))
        .unwrap_or(Doc::nil());
    let value = item
        .children()
        .find(|c| is_expr_node(c.kind()))
        .map(|n| super::exprs::expr(&n))
        .unwrap_or(Doc::nil());
    let has_semi = item
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::SEMI);
    let head = if is_pub { "pub const " } else { "const " };
    let mut out = Doc::concat(
        Doc::text(head),
        Doc::concat(
            name,
            Doc::concat(
                Doc::text(": "),
                Doc::concat(ty, Doc::concat(Doc::text(" = "), value)),
            ),
        ),
    );
    if has_semi {
        out = Doc::concat(out, Doc::text(";"));
    }
    out
}

// ---------------------------------------------------------------------------
// v0.45 T2 — fn / struct / enum / type-alias canonical printers.
//
// Each printer follows the same recipe:
//   1. If the signature region (everything from the leading keyword up to
//      the first body delimiter) contains comments, or the item carries
//      attribute prefixes, fall back to verbatim. The v0.43 const pattern.
//   2. Otherwise build the canonical signature head as a `Doc` and append
//      the body text verbatim from source so statement-level layout +
//      embedded comments inside the body are preserved exactly.
// ---------------------------------------------------------------------------

fn fn_decl_or_verbatim(item: &SyntaxNode, stripped: &str) -> Doc {
    if has_attr_child(item) {
        return Doc::text(stripped.to_string());
    }
    // `requires <expr>` clauses sit at FN_DECL scope between the
    // signature head and the body. Until v0.45 T2's printer can handle
    // them canonically, keep such fns verbatim.
    if item
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::REQUIRES_KW)
    {
        return Doc::text(stripped.to_string());
    }
    // Find the body split point. We look at FN_DECL's direct children +
    // direct tokens only — the BLOCK node for `fn f() { ... }`, the
    // direct `=` token for `fn f() = expr;`, or the direct `;` for an
    // extern-/trait-method signature. Crucially, we do NOT match braces
    // inside nested nodes like EFFECT_CLAUSE (`!{| E}`).
    let split = fn_body_split(item);
    let head_text = match split {
        Some(off) => &stripped[..off.min(stripped.len())],
        None => stripped,
    };
    if head_has_comment(head_text) {
        return Doc::text(stripped.to_string());
    }
    let Some(sig) = fn_signature_doc(item) else {
        return Doc::text(stripped.to_string());
    };
    match split {
        Some(off) if off <= stripped.len() => {
            let tail = &stripped[off..];
            // The split sits at the body delimiter. The canonical form
            // joins signature and body with a single space, e.g.
            // `fn f() -> I32 { ... }`. The tail itself starts with the
            // delimiter character (`{`/`=`/`;`), so prepend a space.
            let glue = if tail.starts_with(';') { "" } else { " " };
            Doc::concat(sig, Doc::text(format!("{glue}{tail}")))
        }
        _ => sig,
    }
}

/// Locate the byte offset (relative to the FN_DECL's text) where the
/// body region begins: either the start of a direct BLOCK child, or the
/// position of a direct `=` token (for `fn f() = expr;`), or the direct
/// `;` token (signature-only fn).
fn fn_body_split(item: &SyntaxNode) -> Option<usize> {
    let base = u32::from(item.text_range().start());
    let body_block = item
        .children()
        .find(|c| c.kind() == SyntaxKind::BLOCK)
        .map(|c| u32::from(c.text_range().start()));
    let direct_eq_or_semi = item
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| matches!(t.kind(), SyntaxKind::EQ | SyntaxKind::SEMI))
        .map(|t| u32::from(t.text_range().start()));
    let best = match (body_block, direct_eq_or_semi) {
        (Some(b), Some(e)) => Some(b.min(e)),
        (Some(b), None) => Some(b),
        (None, Some(e)) => Some(e),
        (None, None) => None,
    };
    best.map(|abs| (abs - base) as usize)
}

fn struct_decl_or_verbatim(item: &SyntaxNode, stripped: &str) -> Doc {
    if has_attr_child(item) {
        return Doc::text(stripped.to_string());
    }
    let split = direct_token_offset(item, SyntaxKind::L_BRACE);
    let head_text = match split {
        Some(off) => &stripped[..off.min(stripped.len())],
        None => stripped,
    };
    if head_has_comment(head_text) {
        return Doc::text(stripped.to_string());
    }
    let Some(sig) = record_signature_doc(item, "struct") else {
        return Doc::text(stripped.to_string());
    };
    match split {
        Some(off) if off <= stripped.len() => {
            let tail = &stripped[off..];
            Doc::concat(sig, Doc::text(format!(" {tail}")))
        }
        // Structs with no body (`struct Foo`) — rare; emit signature only.
        _ => sig,
    }
}

fn enum_decl_or_verbatim(item: &SyntaxNode, stripped: &str) -> Doc {
    if has_attr_child(item) {
        return Doc::text(stripped.to_string());
    }
    let split = direct_token_offset(item, SyntaxKind::L_BRACE);
    let head_text = match split {
        Some(off) => &stripped[..off.min(stripped.len())],
        None => stripped,
    };
    if head_has_comment(head_text) {
        return Doc::text(stripped.to_string());
    }
    let Some(sig) = record_signature_doc(item, "enum") else {
        return Doc::text(stripped.to_string());
    };
    match split {
        Some(off) if off <= stripped.len() => {
            let tail = &stripped[off..];
            Doc::concat(sig, Doc::text(format!(" {tail}")))
        }
        _ => sig,
    }
}

fn type_alias_or_verbatim(item: &SyntaxNode, stripped: &str) -> Doc {
    if has_attr_child(item) {
        return Doc::text(stripped.to_string());
    }
    if head_has_comment(stripped) {
        return Doc::text(stripped.to_string());
    }
    type_alias_doc(item).unwrap_or_else(|| Doc::text(stripped.to_string()))
}

// --- Per-kind signature builders -------------------------------------------

fn fn_signature_doc(item: &SyntaxNode) -> Option<Doc> {
    let is_pub = item.children().any(|c| c.kind() == SyntaxKind::VISIBILITY);
    let is_unsafe = item
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::UNSAFE_KW);
    let name = item
        .children()
        .find(|c| c.kind() == SyntaxKind::NAME)
        .map(|n| Doc::text(n.text().to_string()))?;
    let generics = item
        .children()
        .find(|c| c.kind() == SyntaxKind::GENERIC_PARAM_LIST)
        .map(generic_params_doc)
        .unwrap_or(Doc::nil());
    let params = item
        .children()
        .find(|c| c.kind() == SyntaxKind::FN_PARAM_LIST)
        .map(fn_param_list_doc)
        .unwrap_or(Doc::text("()"));
    let ret = item
        .children()
        .find(|c| c.kind() == SyntaxKind::RET_TYPE)
        .map(|n| {
            let ty = n
                .children()
                .next()
                .map(|t| super::types::type_expr(&t))
                .unwrap_or(Doc::nil());
            Doc::concat(Doc::text(" -> "), ty)
        })
        .unwrap_or(Doc::nil());
    // EFFECT_CLAUSE source text often absorbs trailing trivia (the
    // parser bumps R_BRACE then `finish_node`, but the next token's
    // trivia is attached as a child WHITESPACE token of the same
    // node). Trim it so the canonical glue (` { body }`) doesn't
    // double-space.
    let effect = item
        .children()
        .find(|c| c.kind() == SyntaxKind::EFFECT_CLAUSE)
        .map(|n| {
            let raw = n.text().to_string();
            Doc::concat(Doc::text(" "), Doc::text(raw.trim().to_string()))
        })
        .unwrap_or(Doc::nil());

    let mut head = String::new();
    if is_pub {
        head.push_str("pub ");
    }
    if is_unsafe {
        head.push_str("unsafe ");
    }
    head.push_str("fn ");
    Some(Doc::concat_all([
        Doc::text(head),
        name,
        generics,
        params,
        ret,
        effect,
    ]))
}

fn record_signature_doc(item: &SyntaxNode, keyword: &str) -> Option<Doc> {
    let is_pub = item.children().any(|c| c.kind() == SyntaxKind::VISIBILITY);
    let name = item
        .children()
        .find(|c| c.kind() == SyntaxKind::NAME)
        .map(|n| Doc::text(n.text().to_string()))?;
    let generics = item
        .children()
        .find(|c| c.kind() == SyntaxKind::GENERIC_PARAM_LIST)
        .map(generic_params_doc)
        .unwrap_or(Doc::nil());
    let mut head = String::new();
    if is_pub {
        head.push_str("pub ");
    }
    head.push_str(keyword);
    head.push(' ');
    Some(Doc::concat_all([Doc::text(head), name, generics]))
}

fn type_alias_doc(item: &SyntaxNode) -> Option<Doc> {
    let is_pub = item.children().any(|c| c.kind() == SyntaxKind::VISIBILITY);
    let name = item
        .children()
        .find(|c| c.kind() == SyntaxKind::NAME)
        .map(|n| Doc::text(n.text().to_string()))?;
    let generics = item
        .children()
        .find(|c| c.kind() == SyntaxKind::GENERIC_PARAM_LIST)
        .map(generic_params_doc)
        .unwrap_or(Doc::nil());
    let ty = item
        .children()
        .find(|c| is_type_node(c.kind()))
        .map(|n| super::types::type_expr(&n))?;
    let has_semi = item
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::SEMI);
    let head = if is_pub { "pub type " } else { "type " };
    let mut out = Doc::concat_all([Doc::text(head), name, generics, Doc::text(" = "), ty]);
    if has_semi {
        out = Doc::concat(out, Doc::text(";"));
    }
    Some(out)
}

fn generic_params_doc(list: SyntaxNode) -> Doc {
    let parts: Vec<Doc> = list
        .children()
        .filter(|c| c.kind() == SyntaxKind::GENERIC_PARAM)
        .map(generic_param_doc)
        .collect();
    Doc::concat(
        Doc::text("["),
        Doc::concat(Doc::join(Doc::text(", "), parts), Doc::text("]")),
    )
}

fn generic_param_doc(param: SyntaxNode) -> Doc {
    // GENERIC_PARAM: NAME ( ':' type ( '+' type )* )?
    let name = param
        .children()
        .find(|c| c.kind() == SyntaxKind::NAME)
        .map(|n| Doc::text(n.text().to_string()))
        .unwrap_or(Doc::nil());
    let bounds: Vec<Doc> = param
        .children()
        .filter(|c| is_type_node(c.kind()))
        .map(|t| super::types::type_expr(&t))
        .collect();
    if bounds.is_empty() {
        name
    } else {
        Doc::concat(
            name,
            Doc::concat(Doc::text(": "), Doc::join(Doc::text(" + "), bounds)),
        )
    }
}

fn fn_param_list_doc(list: SyntaxNode) -> Doc {
    // Preserve multi-line param lists verbatim — corpus files often
    // wrap many-arg fns one-param-per-line for diff hygiene, and the
    // single-line canonical form loses that intent. We detect the
    // multi-line shape by looking for a newline anywhere inside the
    // list and, when present, emit the source text unchanged.
    if list.text().to_string().contains('\n') {
        // Same trailing-trivia caveat as EFFECT_CLAUSE: the parser
        // attaches the following whitespace token as a child of the
        // FN_PARAM_LIST node. Trim so the canonical glue around it
        // ("  ->" / "  {") doesn't double-space.
        let raw = list.text().to_string();
        return Doc::text(raw.trim_end().to_string());
    }
    let parts: Vec<Doc> = list
        .children()
        .filter_map(|c| match c.kind() {
            SyntaxKind::FN_PARAM => Some(fn_param_doc(&c)),
            SyntaxKind::VARIADIC_MARKER => Some(Doc::text("...")),
            _ => None,
        })
        .collect();
    // Preserve a trailing comma if the source had one — keeps the
    // non-trivia token stream byte-identical for corpus files that
    // wrote a trailing comma to allow per-line diffs.
    let has_trailing_comma = source_has_trailing_comma(&list, SyntaxKind::FN_PARAM_LIST);
    let inner = if parts.is_empty() {
        Doc::nil()
    } else {
        let joined = Doc::join(Doc::text(", "), parts);
        if has_trailing_comma {
            Doc::concat(joined, Doc::text(","))
        } else {
            joined
        }
    };
    Doc::concat(Doc::text("("), Doc::concat(inner, Doc::text(")")))
}

/// True if the source had a trailing comma after the last item in the
/// list — i.e. between the final element node and the closing
/// delimiter. Walks the list's `children_with_tokens` in reverse,
/// skipping the trailing delimiter + trivia, and checks whether the
/// next non-trivia element is a COMMA token rather than a child node.
fn source_has_trailing_comma(list: &SyntaxNode, _kind: SyntaxKind) -> bool {
    let mut saw_close = false;
    for el in list
        .children_with_tokens()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        match el {
            rowan::NodeOrToken::Token(t) => {
                if t.kind().is_trivia() {
                    continue;
                }
                if !saw_close {
                    // Expect a closing delimiter as the first non-trivia
                    // token from the right.
                    saw_close = true;
                    continue;
                }
                return t.kind() == SyntaxKind::COMMA;
            }
            rowan::NodeOrToken::Node(_) => {
                // Hit an element node before any comma → no trailing
                // comma in the source.
                return false;
            }
        }
    }
    false
}

fn fn_param_doc(param: &SyntaxNode) -> Doc {
    // FN_PARAM: ( SELF_KW | NAME ) ( ':' type )?
    // (Attributes inside FN_PARAM keep the param verbatim — same safety
    // rule as the FN_DECL outer guard.)
    let raw = param.text().to_string();
    if raw.trim_start().starts_with('#') || raw.contains("//") || raw.contains("/*") {
        return Doc::text(raw.trim().to_string());
    }
    let is_self = param
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::SELF_KW);
    let name = if is_self {
        Doc::text("self")
    } else {
        param
            .children()
            .find(|c| c.kind() == SyntaxKind::NAME)
            .map(|n| Doc::text(n.text().to_string()))
            .unwrap_or(Doc::nil())
    };
    let ty = param
        .children()
        .find(|c| is_type_node(c.kind()))
        .map(|n| super::types::type_expr(&n));
    match ty {
        Some(t) => Doc::concat(name, Doc::concat(Doc::text(": "), t)),
        None => name,
    }
}

// --- Helpers ---------------------------------------------------------------

fn has_attr_child(item: &SyntaxNode) -> bool {
    item.children()
        .any(|c| matches!(c.kind(), SyntaxKind::ATTR | SyntaxKind::TOOL_ATTR))
}

fn head_has_comment(s: &str) -> bool {
    s.contains("//") || s.contains("/*")
}

/// Return the byte offset, relative to the item's text, of the first
/// *direct-child* token whose kind matches `kind`. Direct children only —
/// tokens inside nested nodes are skipped. Used to find body delimiters
/// like the `{` that follows a struct/enum signature, without matching
/// a `{` that's nested inside an effect clause's `!{...}` body.
fn direct_token_offset(item: &SyntaxNode, kind: SyntaxKind) -> Option<usize> {
    let base = u32::from(item.text_range().start());
    let tok = item
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == kind)?;
    let abs = u32::from(tok.text_range().start());
    Some((abs - base) as usize)
}

fn is_type_node(k: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(
        k,
        TYPE_PATH
            | TYPE_REF
            | TYPE_BORROW
            | TYPE_TUPLE
            | TYPE_ARRAY
            | TYPE_FN
            | TYPE_DYN
            | TYPE_RESULT_SUGAR
            | TYPE_UNION
    )
}

fn is_expr_node(k: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(
        k,
        LITERAL_EXPR
            | PATH_EXPR
            | BINARY_EXPR
            | UNARY_EXPR
            | POSTFIX_EXPR
            | CALL_EXPR
            | METHOD_CALL_EXPR
            | INDEX_EXPR
            | FIELD_EXPR
            | CAST_EXPR
            | IF_EXPR
            | MATCH_EXPR
            | FOR_EXPR
            | WHILE_EXPR
            | LOOP_EXPR
            | RETURN_EXPR
            | BREAK_EXPR
            | CONTINUE_EXPR
            | YIELD_EXPR
            | TUPLE_EXPR
            | ARRAY_EXPR
            | STRUCT_EXPR
            | MAP_EXPR
            | LAMBDA_EXPR
            | SEND_EXPR
            | ASK_EXPR
            | DEADLINE_EXPR
            | QUESTION_EXPR
            | HTML_EXPR
            | MOVE_EXPR
            | BORROW_EXPR
            | SPAWN_EXPR
            | RUN_EXPR
    )
}

/// Returns true if the given trivia text contains at least one blank
/// line (i.e. two or more `\n` characters).
fn trivia_has_blank_line(trivia: &str) -> bool {
    trivia.matches('\n').count() >= 2
}

/// Extract only the comment lines (line + block comments) from a chunk
/// of trivia text. Returns an empty string if no comments are present.
fn extract_comment_lines(trivia: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in trivia.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
            out.push(line);
        }
    }
    out.join("\n")
}

/// Returns true if `k` is a top-level item node kind. Exposed for
/// future tools that want to walk top-level items.
pub fn is_item_kind(k: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(
        k,
        FN_DECL
            | STRUCT_DECL
            | ENUM_DECL
            | TYPE_ALIAS
            | IMPL_BLOCK
            | TRAIT_DECL
            | CONST_DECL
            | USE_DECL
            | MOD_DECL
            | PACKAGE_DECL
            | AGENT_DECL
            | PROTOCOL_DECL
            | SUPERVISOR_DECL
            | EXTERN_BLOCK
            | EXPORT_DECL
            | MACRO_DECL
    )
}
