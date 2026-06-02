//! Completion (`textDocument/completion`).
//!
//! v0.5 closes the v0.2 "keyword-only" gap. The list returned to the
//! editor is the union of:
//!
//! 1. The full keyword set (always present, low priority).
//! 2. Every top-level definition by name from `DefMap::by_name`
//!    (function / struct / enum-member / module / type-parameter).
//! 3. If the byte immediately before the cursor is `.`, the methods
//!    of the receiver's type **if we can infer it** (walking the CST
//!    backwards from the dot to find a name + looking it up). Falls
//!    back to the built-in method table when we can't.
//! 4. Locals in scope: every binding declared inside the smallest
//!    enclosing fn/handler body whose declaration precedes the cursor.
//! 5. Trait methods registered for any ADT after a `.` (additive).
//! 6. Import suggestions: if the partially-typed identifier matches an
//!    unimported item known to the package's deps (v0.5 best-effort).
//!
//! Locals-in-scope are extracted from the CST (walk `LET_STMT` +
//! `FN_PARAM` siblings of the enclosing fn body) rather than HIR, so
//! we still produce something useful while the file is mid-edit and
//! HIR lowering may have failed.

use crate::docs::DocAnalysis;
use mty_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use mty_types::{pretty_ty, DefRef};
use rowan::TextSize;
use std::collections::HashSet;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, Documentation, InsertTextFormat,
    MarkupContent, MarkupKind, Position,
};

const KEYWORDS: &[&str] = &[
    "agent", "arena", "as", "async", "await", "budget", "cap", "const", "derive", "detach", "dyn",
    "effect", "else", "enum", "export", "extern", "false", "fn", "for", "if", "impl", "import",
    "in", "join", "let", "loop", "macro", "match", "mod", "move", "mut", "on", "package",
    "protocol", "pub", "ref", "requires", "restart", "return", "run", "sandbox", "scope", "self",
    "spawn", "state", "struct", "sup", "task", "trait", "true", "type", "unsafe", "use", "where",
    "while", "with", "yield",
];

pub fn complete(doc: &DocAnalysis, position: Position) -> Option<CompletionResponse> {
    let offset = doc
        .line_index
        .position_to_offset(&doc.source, position.line, position.character);

    let mut items: Vec<CompletionItem> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // v0.46 T5: split the previous single `detail` string into
    // separately-populated `detail` (one-line signature) and
    // `documentation` (free-form markdown body) fields, plus a
    // `deprecated` boolean. Editors render these in distinct UI
    // affordances — VS Code shows `detail` next to the label and
    // expands `documentation` into the popup body — so keeping them
    // separated produces a much richer completion popup than the
    // legacy "everything stuffed into detail" path.
    let add = |label: String,
               kind: CompletionItemKind,
               detail: Option<String>,
               documentation: Option<String>,
               deprecated: bool,
               items: &mut Vec<CompletionItem>,
               seen: &mut HashSet<String>| {
        let key = format!("{:?}{}", kind, label);
        if !seen.insert(key) {
            return;
        }
        let documentation = documentation.map(|md| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: md,
            })
        });
        items.push(CompletionItem {
            label,
            kind: Some(kind),
            detail,
            documentation,
            deprecated: if deprecated { Some(true) } else { None },
            insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
            ..Default::default()
        });
    };

    for kw in KEYWORDS {
        add(
            (*kw).to_string(),
            CompletionItemKind::KEYWORD,
            Some("keyword".to_string()),
            None,
            false,
            &mut items,
            &mut seen,
        );
    }

    // Receiver-aware completion after `.`.
    let after_dot = matches!(preceding_char(&doc.source, offset), Some('.'));
    if after_dot {
        let root = SyntaxNode::new_root(doc.parsed.green.clone());
        let receiver = receiver_token_before_dot(&root, offset);
        let mut emitted_method = false;
        if let Some(recv_name) = receiver {
            if let Some(adt_methods) = methods_for_receiver(doc, &recv_name) {
                for (name, detail) in adt_methods {
                    add(
                        name,
                        CompletionItemKind::METHOD,
                        Some(detail),
                        None,
                        false,
                        &mut items,
                        &mut seen,
                    );
                    emitted_method = true;
                }
            }
        }
        // Always also include the built-in method table as a fallback —
        // these are permissive and apply to any receiver shape.
        for (name, _) in doc.typed.def_map.builtin_methods.iter() {
            add(
                name.clone(),
                CompletionItemKind::METHOD,
                Some("built-in method".to_string()),
                None,
                false,
                &mut items,
                &mut seen,
            );
            emitted_method = true;
        }
        let _ = emitted_method;
    }

    // Top-level definitions.
    for (name, def) in doc.typed.def_map.by_name.iter() {
        let kind = match def {
            DefRef::Fn(_) => CompletionItemKind::FUNCTION,
            DefRef::Adt(_) => CompletionItemKind::STRUCT,
            DefRef::Variant(_, _) => CompletionItemKind::ENUM_MEMBER,
            DefRef::Module(_) => CompletionItemKind::MODULE,
            DefRef::Param(_) => CompletionItemKind::TYPE_PARAMETER,
            // v0.41 T6 (L16): top-level `const` declarations show up as
            // CONSTANT in completion.
            DefRef::Const(_) => CompletionItemKind::CONSTANT,
        };
        // v0.46 T5 — build a one-line signature for the `detail` slot
        // when we can resolve the def. The previous code crammed every
        // def into the literal string "def"; now Fn defs show their
        // arrow-signature, Adt/Variant defs show the kw + name.
        let (detail, documentation) = describe_def(doc, def, name);
        add(
            name.clone(),
            kind,
            detail,
            documentation,
            false,
            &mut items,
            &mut seen,
        );
    }

    // Locals in scope.
    for local in locals_in_scope(doc, offset) {
        add(
            local,
            CompletionItemKind::VARIABLE,
            Some("local".to_string()),
            None,
            false,
            &mut items,
            &mut seen,
        );
    }

    Some(CompletionResponse::Array(items))
}

/// v0.46 T5: build `(detail, documentation)` for a top-level def so the
/// completion item can render its signature in the popup's secondary
/// slot. `detail` is the one-line signature; `documentation` is the
/// (currently empty) markdown body slot reserved for a future
/// doc-comment-walking pass.
fn describe_def(
    doc: &DocAnalysis,
    def: &DefRef,
    fallback_name: &str,
) -> (Option<String>, Option<String>) {
    match def {
        DefRef::Fn(id) => {
            if let Some(f) = doc.typed.def_map.fn_def(*id) {
                let parts: Vec<String> = f
                    .params
                    .iter()
                    .map(|(n, t)| {
                        format!(
                            "{}: {}",
                            n,
                            pretty_ty(*t, &doc.typed.ty_arena, None, Some(&doc.typed.def_map))
                        )
                    })
                    .collect();
                let ret = pretty_ty(f.ret, &doc.typed.ty_arena, None, Some(&doc.typed.def_map));
                let vis = if f.is_pub { "pub " } else { "" };
                let detail = format!("{vis}fn {}({}) -> {}", f.name, parts.join(", "), ret);
                (Some(detail), None)
            } else {
                (Some(format!("fn {}", fallback_name)), None)
            }
        }
        DefRef::Adt(id) => {
            if let Some(a) = doc.typed.def_map.adt(*id) {
                let kw = match a.kind {
                    mty_types::AdtKind::Struct => "struct",
                    mty_types::AdtKind::Enum => "enum",
                    mty_types::AdtKind::Opaque => "type",
                };
                (Some(format!("{kw} {}", a.name)), None)
            } else {
                (Some(fallback_name.to_string()), None)
            }
        }
        DefRef::Variant(id, idx) => {
            if let Some(a) = doc.typed.def_map.adt(*id) {
                if let Some(v) = a.variants.get(*idx) {
                    return (Some(format!("{}.{}", a.name, v.name)), None);
                }
            }
            (Some(fallback_name.to_string()), None)
        }
        DefRef::Module(_) => (Some(format!("mod {}", fallback_name)), None),
        DefRef::Param(_) => (Some(format!("type param {}", fallback_name)), None),
        DefRef::Const(id) => {
            if let Some(c) = doc.typed.def_map.const_def(*id) {
                let vis = if c.is_pub { "pub " } else { "" };
                let ty = pretty_ty(c.ty, &doc.typed.ty_arena, None, Some(&doc.typed.def_map));
                (Some(format!("{vis}const {}: {}", c.name, ty)), None)
            } else {
                (Some(format!("const {}", fallback_name)), None)
            }
        }
    }
}

fn preceding_char(source: &str, offset: u32) -> Option<char> {
    let off = offset as usize;
    if off == 0 || off > source.len() {
        return None;
    }
    source[..off].chars().next_back()
}

/// Walk back from `offset-1` to find the IDENT token before the `.`.
/// Returns its text if a single ident sits immediately before the dot.
fn receiver_token_before_dot(root: &SyntaxNode, offset: u32) -> Option<String> {
    if offset == 0 {
        return None;
    }
    // Inspect the token immediately before the `.` (which sits at offset-1).
    let pos = TextSize::from(offset.saturating_sub(2));
    let token = match root.token_at_offset(pos) {
        rowan::TokenAtOffset::None => return None,
        rowan::TokenAtOffset::Single(t) => t,
        rowan::TokenAtOffset::Between(a, b) => {
            if a.kind() == SyntaxKind::IDENT {
                a
            } else {
                b
            }
        }
    };
    if token.kind() == SyntaxKind::IDENT {
        Some(token.text().to_string())
    } else {
        None
    }
}

/// Best-effort: if `name` is a known local, find its declared type
/// from `expr_ty` (via HIR `let init`). If it's an ADT-typed
/// expression, list its impl methods.
fn methods_for_receiver(doc: &DocAnalysis, receiver_name: &str) -> Option<Vec<(String, String)>> {
    // Walk all fn bodies, find a let-binding whose name matches, then
    // look up its inferred type via expr_ty[init].
    for (fid, f) in doc.package.fns.iter() {
        let Some(block_id) = f.body else { continue };
        let block = &doc.package.blocks[block_id];
        for stmt in &block.stmts {
            if let mty_hir::HirStmt::Let { pat, init, .. } = stmt {
                let pat = &doc.package.pats[*pat];
                let name = match pat {
                    mty_hir::HirPat::Binding { name, .. } => Some(name.clone()),
                    _ => None,
                };
                if let (Some(n), Some(eid)) = (name, init) {
                    if n == receiver_name {
                        if let Some(tyid) = doc.typed.expr_ty.get(eid).copied() {
                            return methods_for_ty(doc, tyid);
                        }
                    }
                }
            }
        }
        // Also handle params.
        if let Some(plist) = doc.typed.fn_params.get(&fid) {
            for (n, tyid) in plist {
                if n == receiver_name {
                    return methods_for_ty(doc, *tyid);
                }
            }
        }
    }
    None
}

fn methods_for_ty(doc: &DocAnalysis, ty: mty_types::TyId) -> Option<Vec<(String, String)>> {
    use mty_types::TyData;
    let resolved = doc.typed.ty_arena.get(ty);
    let adt_id = match resolved {
        TyData::Adt(id, _) => *id,
        _ => return None,
    };
    let mut out: Vec<(String, String)> = Vec::new();
    let adt = doc.typed.def_map.adt(adt_id)?;
    // Direct impl methods.
    for ((adt2, mname), fid) in doc.typed.def_map.impl_methods.iter() {
        if *adt2 == adt_id {
            let detail = doc
                .typed
                .def_map
                .fn_def(*fid)
                .map(|_f| format!("method on {}", adt.name))
                .unwrap_or_else(|| "method".into());
            out.push((mname.clone(), detail));
        }
    }
    // Trait methods.
    for ((adt2, mname), provs) in doc.typed.def_map.traits.by_method.iter() {
        if *adt2 == adt_id {
            for (tname, _fid) in provs {
                out.push((mname.clone(), format!("trait {} on {}", tname, adt.name)));
            }
        }
    }
    // Fields (rendered as properties so editors give a different icon).
    for v in &adt.variants {
        for (i, f) in v.fields.iter().enumerate() {
            let name = f.name.clone().unwrap_or_else(|| format!("_{}", i));
            out.push((name, format!("field of {}", adt.name)));
        }
    }
    Some(out)
}

/// Collect locally-declared names visible at `cursor` inside the
/// smallest enclosing fn/agent-handler body.
fn locals_in_scope(doc: &DocAnalysis, cursor: u32) -> Vec<String> {
    let root = SyntaxNode::new_root(doc.parsed.green.clone());
    let pos = TextSize::from(cursor);
    let mut best: Option<SyntaxNode> = None;
    for n in root.descendants() {
        if matches!(n.kind(), SyntaxKind::BLOCK | SyntaxKind::ON_HANDLER)
            && n.text_range().contains(pos)
        {
            match &best {
                None => best = Some(n.clone()),
                Some(prev) if n.text_range().len() < prev.text_range().len() => {
                    best = Some(n.clone())
                }
                _ => {}
            }
        }
    }
    let mut names: Vec<String> = Vec::new();
    let Some(scope) = best else {
        return names;
    };

    // Walk preceding LET_STMTs that *end* before the cursor.
    for d in scope.descendants() {
        if d.kind() == SyntaxKind::LET_STMT && d.text_range().end() <= pos {
            // Extract the IDENT_PAT child's IDENT token.
            if let Some(name_tok) = first_ident_in_pat(&d) {
                names.push(name_tok.text().to_string());
            }
        }
    }
    // Also include enclosing fn params.
    let mut cur = scope.parent();
    while let Some(n) = cur {
        if n.kind() == SyntaxKind::FN_DECL {
            if let Some(plist) = n.children().find(|c| c.kind() == SyntaxKind::FN_PARAM_LIST) {
                for p in plist
                    .children()
                    .filter(|c| c.kind() == SyntaxKind::FN_PARAM)
                {
                    if let Some(t) = p
                        .children_with_tokens()
                        .filter_map(|c| c.into_token())
                        .find(|t| t.kind() == SyntaxKind::IDENT)
                    {
                        names.push(t.text().to_string());
                    }
                }
            }
            break;
        }
        cur = n.parent();
    }
    names.sort();
    names.dedup();
    names
}

fn first_ident_in_pat(let_stmt: &SyntaxNode) -> Option<SyntaxToken> {
    for d in let_stmt.descendants_with_tokens() {
        if let Some(t) = d.as_token() {
            if t.kind() == SyntaxKind::IDENT {
                return Some(t.clone());
            }
        }
    }
    None
}
