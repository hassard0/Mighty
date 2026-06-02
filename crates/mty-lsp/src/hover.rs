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
use tower_lsp::lsp_types::{Hover, HoverContents, LanguageString, MarkedString, Position, Range};

/// v0.46 T5 — one structured hover section. Either a code-fenced
/// signature (`Code { language: "mty", value: ... }`) or a markdown
/// body block. The list is rendered as an `HoverContents::Array`, which
/// the IDE's L31 parser and other LSP clients consume as a sequence of
/// individually-rendered cards (signature with syntax highlighting,
/// docs without — no shared `wrap_hover` fence-stripping needed).
enum HoverSection {
    Code { language: String, value: String },
    Markdown(String),
}

impl HoverSection {
    fn into_marked_string(self) -> MarkedString {
        match self {
            HoverSection::Code { language, value } => {
                MarkedString::LanguageString(LanguageString { language, value })
            }
            HoverSection::Markdown(text) => MarkedString::String(text),
        }
    }
}

/// Top-level hover entry.
///
/// Returns `None` if no useful hover information is available at this
/// position (so the client shows nothing rather than an empty box).
///
/// v0.46 T5: the response carries a structured `HoverContents::Array`
/// of [`MarkedString`] sections. The first section (when present) is a
/// language-tagged code block (`{ language: "mty", value: "<sig>" }`)
/// so editors render the signature with syntax highlighting; the
/// remaining sections are markdown bodies (description, capability,
/// example, see-also, debug tags). Clients that only understood the
/// previous single-string markdown blob still receive the same content
/// — `HoverContents::Array` predates the `MarkupContent` shape and
/// every modern LSP client renders both interchangeably.
pub fn hover(doc: &DocAnalysis, position: Position) -> Option<Hover> {
    let offset = doc
        .line_index
        .position_to_offset(&doc.source, position.line, position.character);
    let root = SyntaxNode::new_root(doc.parsed.green.clone());
    let token = token_at_offset(&root, offset)?;
    let token_text = token.text().to_string();
    let token_kind = token.kind();

    let mut sections: Vec<HoverSection> = Vec::new();

    if matches!(token_kind, SyntaxKind::IDENT) {
        let mut rendered_any = false;
        if let Some(sig) = render_named_def_signature(doc, &token_text) {
            sections.push(HoverSection::Code {
                language: "mty".to_string(),
                value: sig,
            });
            rendered_any = true;
        }
        // Try qualified lookup (`Member.anthropic`) by walking up the
        // PATH ancestor and joining segments, then method-call
        // (`r.ask`) by walking up to METHOD_CALL_EXPR.
        if let Some(entry) = stdlib_entry_for_token(&token, &token_text) {
            append_stdlib_sections(&mut sections, entry);
            rendered_any = true;
        }
        if !rendered_any {
            sections.push(HoverSection::Code {
                language: String::new(),
                value: token_text.clone(),
            });
        }
    } else {
        sections.push(HoverSection::Code {
            language: String::new(),
            value: token_text.clone(),
        });
    }

    // Always tag with the surrounding node kind for debuggability.
    let mut debug_lines: Vec<String> = Vec::new();
    if let Some(parent) = token.parent() {
        debug_lines.push(format!("_node_: `{:?}`", parent.kind()));
    }
    debug_lines.push(format!("_token_: `{:?}`", token_kind));
    sections.push(HoverSection::Markdown(debug_lines.join("\n\n")));

    let range = token_range(&token, &doc.line_index, &doc.source);
    let array: Vec<MarkedString> = sections
        .into_iter()
        .map(HoverSection::into_marked_string)
        .collect();
    Some(Hover {
        contents: HoverContents::Array(array),
        range: Some(range),
    })
}

/// v0.46 T5: return the user-defined-symbol's signature as plain text
/// (no code fence). The caller wraps it in a [`MarkedString::LanguageString`]
/// so the editor renders it with `mty` syntax highlighting independently
/// from any markdown body following it.
fn render_named_def_signature(doc: &DocAnalysis, name: &str) -> Option<String> {
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
                "{vis}fn {}({}) -> {}{}",
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
            Some(format!("{kw} {}", a.name))
        }
        DefRef::Variant(id, idx) => {
            let a = doc.typed.def_map.adt(*id)?;
            let v = a.variants.get(*idx)?;
            Some(format!("{}.{}", a.name, v.name))
        }
        DefRef::Module(_) => Some(format!("mod {}", name)),
        DefRef::Param(_) => Some(format!("type param {}", name)),
        DefRef::Const(id) => {
            // v0.41 T6 (L16): hover for a top-level `const NAME: T = ...;`.
            let c = doc.typed.def_map.const_def(*id)?;
            let vis = if c.is_pub { "pub " } else { "" };
            let ty = pretty_ty(c.ty, &doc.typed.ty_arena, None, Some(&doc.typed.def_map));
            Some(format!("{vis}const {}: {}", c.name, ty))
        }
    }
}

/// Try to find a stdlib examples-index entry for the token under the
/// cursor.
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
/// v0.46 T5: returns the resolved [`StdlibExample`] reference rather
/// than pre-rendered markdown — the caller splits the entry into
/// structured `MarkedString` sections.
fn stdlib_entry_for_token(token: &SyntaxToken, token_text: &str) -> Option<&'static StdlibExample> {
    // Path-form lookup (`Member.anthropic`, `std.http.get`).
    if let Some(path_text) = enclosing_path_text(token) {
        if let Some(entry) = lookup_stdlib(&path_text) {
            return Some(entry);
        }
    }
    // Method-call lookup (`receiver.method(...)`).
    if let Some((receiver, method)) = enclosing_method_call(token, token_text) {
        // v0.34 T4 — when the receiver is a local binding (lower-case
        // ident), try to resolve its bound type by searching enclosing
        // scopes for `let <receiver> = <Type>.<ctor>(...)`. This routes
        // `m.ask(...)` where `let m = Member.anthropic("x")` through
        // the `Member.ask` stdlib entry instead of falling through to
        // bare-name `ask` lookup. Mirrors the same ctor-source pattern
        // used by `mty_types::taint`'s receiver-type dispatch.
        if let Some(bound_ty) = resolve_local_bound_type(token, &receiver) {
            if let Some(entry) = lookup_stdlib_method(&bound_ty, &method) {
                return Some(entry);
            }
        }
        if let Some(entry) = lookup_stdlib_method(&receiver, &method) {
            return Some(entry);
        }
    }
    // Bare-name lookup as a last resort.
    lookup_stdlib(token_text)
}

/// v0.46 T5: split a stdlib entry into the structured sections the
/// LSP hover surface exposes — signature in an `mty` code block,
/// description / capability / example / see-also as independent
/// markdown bodies. The IDE's L31 parser was previously forced to
/// `wrap_hover`-strip a single combined markdown blob; the structured
/// shape lets it render each section verbatim.
fn append_stdlib_sections(sections: &mut Vec<HoverSection>, entry: &'static StdlibExample) {
    let sig = entry.signature.trim().to_string();
    if !sig.is_empty() {
        sections.push(HoverSection::Code {
            language: "mty".to_string(),
            value: sig,
        });
    }
    let mut body = String::new();
    if !entry.description.is_empty() {
        body.push_str(entry.description.trim());
        body.push_str("\n\n");
    }
    if !entry.capability.is_empty() {
        body.push_str("**Required capability:** `");
        body.push_str(entry.capability.trim());
        body.push_str("`\n\n");
    }
    if !body.is_empty() {
        // Trim the trailing blank we appended for separator hygiene.
        sections.push(HoverSection::Markdown(body.trim_end().to_string()));
    }
    if !entry.example.is_empty() {
        sections.push(HoverSection::Markdown("**Example:**".to_string()));
        sections.push(HoverSection::Code {
            language: "mty".to_string(),
            value: entry.example.trim_end().to_string(),
        });
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
        let formatted: Vec<String> = see.iter().map(|s| format!("`{}`", s)).collect();
        sections.push(HoverSection::Markdown(format!(
            "**See also:** {}",
            formatted.join(", ")
        )));
    }
}

/// v0.34 T4 — resolve a local binding's "stdlib type" by syntactic
/// search of the enclosing scopes.
///
/// Walks up from `token`'s enclosing block/fn looking for a
/// `LET_STMT` whose binding name matches `local`. When found, inspects
/// the initialiser:
///
/// 1. `let m = Type.ctor(...)` — extract `Type` (e.g. `Member`).
/// 2. `let m = pkg.Type.ctor(...)` — extract `pkg.Type`.
/// 3. `let m = make_thing()` — recurse: look at the called fn's body
///    for a tail-position `Type.ctor(...)` or known `-> Type` return.
///    (v0.34 ships the syntactic-fallback path; full type-driven
///    resolution lands in v0.35.)
///
/// Returns the receiver-type name (e.g. `"Member"`) if known,
/// `None` otherwise.
fn resolve_local_bound_type(token: &SyntaxToken, local: &str) -> Option<String> {
    // The "Type." identifier MUST start with an upper-case letter; we
    // only recognise stdlib-style ctors. Lower-case receivers in
    // `let m = lowercase.thing()` aren't routable through the stdlib
    // examples index.
    let mut node = token.parent()?;
    // Walk up to a containing BLOCK / fn body, scanning each scope for
    // a matching LET_STMT.
    loop {
        // Inspect statements in any encountered block-ish container.
        if node.kind() == SyntaxKind::BLOCK
            || node.kind() == SyntaxKind::FN_DECL
            || node.kind() == SyntaxKind::ON_HANDLER
            || node.kind() == SyntaxKind::LAMBDA_EXPR
        {
            if let Some(ty) = scan_block_for_let_type(&node, local) {
                return Some(ty);
            }
        }
        node = node.parent()?;
    }
}

/// Walk `block`'s descendants looking for a `LET_STMT` that binds
/// `local`. Returns the receiver-type name from the initialiser's
/// constructor call.
fn scan_block_for_let_type(block: &SyntaxNode, local: &str) -> Option<String> {
    for desc in block.descendants() {
        if desc.kind() != SyntaxKind::LET_STMT {
            continue;
        }
        // Pattern child must be a simple ident pattern matching `local`.
        // Mighty's CST surfaces `let x = ...` as a LET_STMT whose first
        // direct child is an IDENT_PAT containing one IDENT token.
        let bound_name = desc
            .children()
            .find(|c| c.kind() == SyntaxKind::IDENT_PAT)
            .and_then(|p| {
                p.descendants_with_tokens()
                    .filter_map(|el| el.into_token())
                    .find(|t| t.kind() == SyntaxKind::IDENT)
                    .map(|t| t.text().to_string())
            });
        if bound_name.as_deref() != Some(local) {
            continue;
        }
        // Find the initialiser expression (a CALL_EXPR whose callee is
        // a PATH_EXPR of shape `Type.ctor` or `pkg.Type.ctor`).
        let init = desc
            .descendants()
            .find(|c| c.kind() == SyntaxKind::CALL_EXPR);
        if let Some(call) = init {
            // The first child of CALL_EXPR is the callee path.
            if let Some(path_expr) = call
                .children()
                .find(|c| c.kind() == SyntaxKind::PATH_EXPR || c.kind() == SyntaxKind::PATH)
            {
                let segs: Vec<String> = path_expr
                    .descendants_with_tokens()
                    .filter_map(|el| el.into_token())
                    .filter(|t| t.kind() == SyntaxKind::IDENT)
                    .map(|t| t.text().to_string())
                    .collect();
                if segs.len() >= 2 {
                    // `Type.ctor` → "Type"; `pkg.Type.ctor` → "pkg.Type".
                    // Heuristic: the LAST segment is the ctor (e.g.
                    // `anthropic`), everything before it is the type
                    // path.
                    let ty_segs = &segs[..segs.len() - 1];
                    let ty = ty_segs.join(".");
                    if !ty.is_empty() {
                        return Some(ty);
                    }
                }
            }
        }
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

/// Legacy single-markdown-blob rendering of a stdlib entry. v0.46 T5
/// moved the live hover surface to structured `HoverSection`s; this
/// remains under `cfg(test)` to keep the v0.33 T6 section regressions
/// (still-useful as content checks) in place without re-deriving them
/// from the structured form.
#[cfg(test)]
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
            | SyntaxKind::HEX_INT_LITERAL
            | SyntaxKind::BIN_INT_LITERAL
            | SyntaxKind::OCT_INT_LITERAL
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
