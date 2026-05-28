//! Hover support — given a `(uri, position)`, return a human-readable
//! markdown blurb describing the token under the cursor.
//!
//! ## Sections (v0.33 T6)
//!
//! Hover output is composed of these stable sections, in order:
//!
//! 1. **Signature** — pretty-printed fn/method signature, fenced as `mty`.
//! 2. **Description** — one- to two-sentence summary.
//! 3. **Required capability** — `net.https`, `fs.read`, etc., when
//!    applicable.
//! 4. **Example** — a `///`-extracted usage block, fenced as `mty`.
//! 5. **See also** — up to five related symbols, comma-separated.
//!
//! The richer-than-v0.2 payload sources its sections from two places:
//!
//! - The user's own `DefMap` (parsed + type-checked by `mty-types`),
//!   for fn/struct/enum/etc declared in the file under the cursor.
//! - The curated **stdlib examples index** in `mty_doc::examples`,
//!   for `std.*` symbols (`Member.ask`, `swarm`, `std.http.get`, ...)
//!   whose implementations live in `mty-stdlib` and therefore can't
//!   be reached by the `///`-walking doc generator.
//!
//! ## Context inference
//!
//! When the cursor sits on a bare method name (`r.ask(...)`), the
//! identifier alone — `ask` — is ambiguous (it could be `Member.ask`,
//! `AgentRef.ask`, or a user-defined trait method). The hover walks
//! up to the surrounding `METHOD_CALL_EXPR` and uses the receiver
//! identifier as a hint: a literal `Member` receiver, an upper-case
//! receiver name, or a known stdlib constructor return all bias the
//! lookup toward the stdlib examples index. The bias is intentionally
//! conservative — when the receiver is a lower-case binding whose type
//! we cannot statically read, we fall back to bare-name lookup, which
//! is still useful for the common cases (`m.ask`, `c.body`, ...).

use crate::docs::DocAnalysis;
use crate::line_index::LineIndex;
use mty_doc::examples::{
    infer_see_also as infer_stdlib_see_also, lookup as lookup_stdlib,
    lookup_method as lookup_stdlib_method, StdlibExample,
};
use mty_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use mty_types::{pretty_ty, DefRef};
use rowan::TextSize;
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Range};

/// Top-level hover entry.
///
/// Returns `None` if no useful hover information is available at this
/// position (so the client shows nothing rather than an empty box).
pub fn hover(doc: &DocAnalysis, position: Position) -> Option<Hover> {
    let offset = doc
        .line_index
        .position_to_offset(&doc.source, position.line, position.character);
    let root = SyntaxNode::new_root(doc.parsed.green.clone());
    let token = token_at_offset(&root, offset)?;
    let token_text = token.text().to_string();
    let token_kind = token.kind();

    let mut sections: Vec<String> = Vec::new();

    // Identifier-style hover: try to look the name up in the DefMap,
    // and *also* try the stdlib examples index. We render both — the
    // DefMap path supplies the user-defined signature when present, and
    // the stdlib index supplies the description/example/see-also for
    // stdlib symbols regardless of whether the user shadowed them.
    if matches!(token_kind, SyntaxKind::IDENT) {
        let mut rendered_any = false;
        if let Some(rendered) = render_named_def(doc, &token_text) {
            sections.push(rendered);
            rendered_any = true;
        }
        // Try qualified lookup (`Member.anthropic`) by walking up the
        // PATH ancestor and joining segments, then method-call
        // (`r.ask`) by walking up to METHOD_CALL_EXPR.
        if let Some(stdlib_md) = stdlib_hover_for_token(&token, &token_text) {
            sections.push(stdlib_md);
            rendered_any = true;
        }
        if !rendered_any {
            sections.push(format!("```\n{}\n```", token_text));
        }
    } else {
        // Show the literal token text in a code fence so syntax tokens
        // (keywords, literals) still get a friendly box.
        sections.push(format!("```\n{}\n```", token_text));
    }

    // Always tag with the surrounding node kind for debuggability.
    if let Some(parent) = token.parent() {
        sections.push(format!("_node_: `{:?}`", parent.kind()));
    }
    sections.push(format!("_token_: `{:?}`", token_kind));

    let range = token_range(&token, &doc.line_index, &doc.source);
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: sections.join("\n\n"),
        }),
        range: Some(range),
    })
}

fn render_named_def(doc: &DocAnalysis, name: &str) -> Option<String> {
    let def = doc.typed.def_map.by_name.get(name)?;
    match def {
        DefRef::Fn(id) => {
            let f = doc.typed.def_map.fn_def(*id)?;
            let params: Vec<String> = f
                .params
                .iter()
                .map(|(pn, t)| {
                    format!(
                        "{}: {}",
                        pn,
                        pretty_ty(*t, &doc.typed.ty_arena, None, Some(&doc.typed.def_map))
                    )
                })
                .collect();
            let ret = pretty_ty(f.ret, &doc.typed.ty_arena, None, Some(&doc.typed.def_map));
            let vis = if f.is_pub { "pub " } else { "" };
            let effects = if f.effects.is_empty() {
                String::new()
            } else {
                " effect <...>".to_string()
            };
            Some(format!(
                "```mty\n{vis}fn {}({}) -> {}{}\n```",
                f.name,
                params.join(", "),
                ret,
                effects
            ))
        }
        DefRef::Adt(id) => {
            let a = doc.typed.def_map.adt(*id)?;
            let kw = match a.kind {
                mty_types::AdtKind::Struct => "struct",
                mty_types::AdtKind::Enum => "enum",
                mty_types::AdtKind::Opaque => "type",
            };
            Some(format!("```mty\n{kw} {}\n```", a.name))
        }
        DefRef::Variant(id, idx) => {
            let a = doc.typed.def_map.adt(*id)?;
            let v = a.variants.get(*idx)?;
            Some(format!("```mty\n{}.{}\n```", a.name, v.name))
        }
        DefRef::Module(_) => Some(format!("```mty\nmod {}\n```", name)),
        DefRef::Param(_) => Some(format!("```mty\ntype param {}\n```", name)),
    }
}

/// Try to find a stdlib examples-index entry for the token under the
/// cursor and render it as the rich-markdown payload (signature,
/// description, capability, example, see-also).
///
/// Resolution order:
///
/// 1. Walk up to PATH/PATH_EXPR; join segments separated by `.`. If
///    the joined name resolves in the index, return it.
/// 2. Walk up to METHOD_CALL_EXPR; if the receiver child is itself a
///    PATH whose head looks like a type name (upper-case head), try
///    `<receiver>.<token>`. Otherwise fall back to bare-method lookup.
/// 3. Bare-name lookup on `token` (e.g. hover on `log`).
///
/// Returns markdown ready to drop into the hover sections list.
fn stdlib_hover_for_token(token: &SyntaxToken, token_text: &str) -> Option<String> {
    // Path-form lookup (`Member.anthropic`, `std.http.get`).
    if let Some(path_text) = enclosing_path_text(token) {
        if let Some(entry) = lookup_stdlib(&path_text) {
            return Some(render_stdlib_entry(entry));
        }
    }
    // Method-call lookup (`receiver.method(...)`).
    if let Some((receiver, method)) = enclosing_method_call(token, token_text) {
        if let Some(entry) = lookup_stdlib_method(&receiver, &method) {
            return Some(render_stdlib_entry(entry));
        }
    }
    // Bare-name lookup as a last resort.
    if let Some(entry) = lookup_stdlib(token_text) {
        return Some(render_stdlib_entry(entry));
    }
    None
}

/// If the token is inside a PATH (e.g. `Member.anthropic`), reconstruct
/// the joined dotted path text. Returns `None` when no PATH ancestor is
/// found or when the path doesn't contain at least one `.`.
fn enclosing_path_text(token: &SyntaxToken) -> Option<String> {
    let mut node = token.parent()?;
    loop {
        if node.kind() == SyntaxKind::PATH || node.kind() == SyntaxKind::PATH_EXPR {
            let segments: Vec<String> = node
                .descendants_with_tokens()
                .filter_map(|el| el.into_token())
                .filter(|t| t.kind() == SyntaxKind::IDENT)
                .map(|t| t.text().to_string())
                .collect();
            if segments.len() >= 2 {
                return Some(segments.join("."));
            }
            return None;
        }
        node = node.parent()?;
    }
}

/// If the token sits in a METHOD_CALL_EXPR's name slot, return
/// `(receiver, method_name)` where `receiver` is the receiver's source
/// text (best effort — useful when it's a literal type name like
/// `Member`). Lower-case receivers (variables) still work because the
/// caller falls back to bare-method lookup.
fn enclosing_method_call(token: &SyntaxToken, token_text: &str) -> Option<(String, String)> {
    // Walk up to a METHOD_CALL_EXPR.
    let mut node = token.parent()?;
    let call = loop {
        if node.kind() == SyntaxKind::METHOD_CALL_EXPR {
            break node;
        }
        node = node.parent()?;
    };
    // The first child of METHOD_CALL_EXPR is the receiver expression;
    // the method name is an IDENT trailing the `.`. We do a permissive
    // scan: grab the first IDENT token in the receiver subtree (for
    // simple `Receiver.method()` cases that's the head) and trust the
    // hovered token as the method name.
    let mut children = call.children_with_tokens();
    let receiver_node = children.find_map(|el| el.into_node())?;
    let receiver_head = receiver_node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT)?
        .text()
        .to_string();
    Some((receiver_head, token_text.to_string()))
}

/// Render a stdlib examples entry as the markdown payload spliced into
/// the hover output. The curated `see_also` field is rendered verbatim;
/// inferred siblings (same family / same module / same capability) are
/// appended after the curated list, up to five total.
fn render_stdlib_entry(entry: &'static StdlibExample) -> String {
    let mut md = String::new();
    md.push_str("```mty\n");
    md.push_str(entry.signature.trim());
    md.push_str("\n```\n\n");
    if !entry.description.is_empty() {
        md.push_str(entry.description.trim());
        md.push_str("\n\n");
    }
    if !entry.capability.is_empty() {
        md.push_str("**Required capability:** `");
        md.push_str(entry.capability.trim());
        md.push_str("`\n\n");
    }
    if !entry.example.is_empty() {
        md.push_str("**Example:**\n\n```mty\n");
        md.push_str(entry.example.trim_end());
        md.push_str("\n```\n\n");
    }

    // Merge curated + inferred see-also, deduped, capped at 5.
    let mut see: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for s in entry.see_also_iter() {
        if see.len() >= 5 {
            break;
        }
        if seen.insert(s.to_string()) {
            see.push(s.to_string());
        }
    }
    if see.len() < 5 {
        for sym in infer_stdlib_see_also(entry, 5 - see.len()) {
            if see.len() >= 5 {
                break;
            }
            if seen.insert(sym.to_string()) {
                see.push(sym.to_string());
            }
        }
    }
    if !see.is_empty() {
        md.push_str("**See also:** ");
        let formatted: Vec<String> = see.iter().map(|s| format!("`{}`", s)).collect();
        md.push_str(&formatted.join(", "));
        md.push('\n');
    }
    md
}

fn token_at_offset(root: &SyntaxNode, offset: u32) -> Option<SyntaxToken> {
    let pos = TextSize::from(offset);
    let len = root.text_range().len();
    let pos = if pos >= len {
        len.checked_sub(TextSize::from(1))?
    } else {
        pos
    };
    match root.token_at_offset(pos) {
        rowan::TokenAtOffset::None => None,
        rowan::TokenAtOffset::Single(t) => Some(t),
        rowan::TokenAtOffset::Between(a, b) => {
            if is_interesting(a.kind()) {
                Some(a)
            } else {
                Some(b)
            }
        }
    }
}

fn is_interesting(k: SyntaxKind) -> bool {
    matches!(
        k,
        SyntaxKind::IDENT
            | SyntaxKind::INT_LITERAL
            | SyntaxKind::FLOAT_LITERAL
            | SyntaxKind::STRING_LITERAL
            | SyntaxKind::CHAR_LITERAL
            | SyntaxKind::DURATION_LITERAL
            | SyntaxKind::SIZE_LITERAL
            | SyntaxKind::HTML_LITERAL
    )
}

fn token_range(token: &SyntaxToken, line_index: &LineIndex, source: &str) -> Range {
    let r = token.text_range();
    crate::conv::span_to_range(line_index, source, r.start().into(), r.end().into())
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use mty_doc::examples::lookup;

    #[test]
    fn render_member_ask_has_all_sections() {
        let e = lookup("Member.ask").expect("seeded");
        let md = render_stdlib_entry(e);
        assert!(md.contains("```mty"), "missing code fence: {md}");
        assert!(md.contains("Required capability"), "missing capability");
        assert!(md.contains("Example:"));
        assert!(md.contains("See also:"));
        assert!(md.contains("Member.anthropic") || md.contains("Member.openai"));
    }

    #[test]
    fn render_log_has_no_capability_section() {
        let e = lookup("log").expect("seeded");
        let md = render_stdlib_entry(e);
        assert!(!md.contains("Required capability"));
        assert!(md.contains("Example:"));
    }
}
