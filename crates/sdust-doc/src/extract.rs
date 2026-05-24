//! Doc-comment extraction.
//!
//! Pipeline:
//!
//! 1. Run [`sdust_driver::parse_source`] + [`sdust_driver::lower`] on
//!    the input source.
//! 2. Walk the CST top-level. For each item-bearing child, harvest
//!    immediately-preceding `///` doc-comment trivia tokens.
//! 3. The CST's first top-level child can also be preceded by `//!`
//!    bang-doc tokens that belong to the package, not the item.
//! 4. Render a signature line per item directly from the HIR.
//! 5. Split the doc body into synopsis (first sentence), examples
//!    (fenced code blocks tagged `sd`/`stardust`), and a remaining
//!    CommonMark body.
//! 6. Compute simple intra-package back-links by scanning expression
//!    bodies for [`sdust_hir::HirExpr::Path`] hits that resolve to a
//!    documented item.

use crate::ir::*;
use rowan::NodeOrToken;
use sdust_ast::{AstNode, File};
use sdust_diagnostics::{codes::UNEXPECTED_TOKEN, Diagnostic, Label};
use sdust_hir::*;
use sdust_syntax::{parse, SyntaxKind, SyntaxNode, SyntaxToken};

/// Build a [`DocPackage`] from a single source file.
///
/// Returns the IR plus any diagnostics emitted by the underlying
/// parser/lowerer. Diagnostics are returned (not panicked-on) so the
/// CLI can decide whether to abort or to render docs with caveats.
pub fn build_doc_package(
    source: &str,
    _source_id: &str,
    default_name: &str,
) -> (DocPackage, Vec<Diagnostic>) {
    let r = parse(source);
    let mut diags: Vec<Diagnostic> = r
        .errors
        .iter()
        .map(|e| {
            Diagnostic::error(
                UNEXPECTED_TOKEN,
                Label {
                    start: e.start,
                    end: e.end,
                    message: e.message.clone(),
                },
            )
        })
        .collect();
    let file = File::cast(SyntaxNode::new_root(r.green.clone()))
        .expect("FILE root after parse");
    let (pkg, lower_diags) = sdust_hir::lower::LoweringCtx::new().lower_file(file.clone());
    diags.extend(lower_diags);

    let pkg_name = file_package_name(&file).unwrap_or_else(|| default_name.to_string());
    let (file_synopsis, file_body) = extract_file_doc(&file);

    let mut doc = DocPackage {
        name: pkg_name,
        version: "0.0.0".to_string(),
        synopsis: file_synopsis,
        body: file_body,
        modules: Vec::new(),
        items: Vec::new(),
    };

    // Walk top-level item nodes in source order; attach doc comments.
    for item_node in file.0.children() {
        let raw = collect_doc_comments(&item_node);
        let raw_text = doc_block_text(&raw);
        match item_node.kind() {
            SyntaxKind::FN_DECL => {
                if let Some(item) = doc_from_fn(&pkg, &item_node, &raw_text) {
                    doc.items.push(item);
                }
            }
            SyntaxKind::STRUCT_DECL => {
                if let Some(item) = doc_from_struct(&pkg, &item_node, &raw_text) {
                    doc.items.push(item);
                }
            }
            SyntaxKind::ENUM_DECL => {
                if let Some(item) = doc_from_enum(&pkg, &item_node, &raw_text) {
                    doc.items.push(item);
                }
            }
            SyntaxKind::TYPE_ALIAS => {
                if let Some(item) = doc_from_type_alias(&pkg, &item_node, &raw_text) {
                    doc.items.push(item);
                }
            }
            SyntaxKind::AGENT_DECL => {
                if let Some(item) = doc_from_agent(&pkg, &item_node, &raw_text) {
                    doc.items.push(item);
                }
            }
            SyntaxKind::PROTOCOL_DECL => {
                if let Some(item) = doc_from_protocol(&pkg, &item_node, &raw_text) {
                    doc.items.push(item);
                }
            }
            SyntaxKind::SUPERVISOR_DECL => {
                if let Some(item) = doc_from_supervisor(&pkg, &item_node, &raw_text) {
                    doc.items.push(item);
                }
            }
            SyntaxKind::TRAIT_DECL => {
                if let Some(item) = doc_from_trait(&pkg, &item_node, &raw_text) {
                    doc.items.push(item);
                }
            }
            SyntaxKind::MOD_DECL => {
                if let Some(m) = doc_from_mod(&item_node, &raw_text) {
                    doc.modules.push(m);
                }
            }
            _ => {}
        }
    }

    compute_backlinks(&pkg, &mut doc);

    (doc, diags)
}

// ---------------------------------------------------------------------------
// Comment extraction
// ---------------------------------------------------------------------------

/// Collect `///` doc comments adjacent to `item_node`. Walks
/// backwards across ALL leaf tokens (regardless of node nesting),
/// since the parser tends to absorb whitespace + doc-comment trivia
/// into the trailing edge of the previous item's deepest leaf node
/// (e.g. inside its PATH).
fn collect_doc_comments(item_node: &SyntaxNode) -> Vec<SyntaxToken> {
    let Some(first) = item_node.first_token() else {
        return Vec::new();
    };
    // Walk backwards through ALL leaf tokens (regardless of node
    // nesting) until we leave the trivia zone.
    let mut rev: Vec<SyntaxToken> = Vec::new();
    let mut cursor = first.prev_token();
    while let Some(t) = cursor {
        match t.kind() {
            SyntaxKind::WHITESPACE
            | SyntaxKind::DOC_COMMENT
            | SyntaxKind::LINE_COMMENT
            | SyntaxKind::BLOCK_COMMENT => {
                rev.push(t.clone());
                cursor = t.prev_token();
            }
            _ => break,
        }
    }
    rev.reverse();
    extract_doc_run(&rev)
}

/// From a stream of trivia tokens (in source order, ending right
/// before the item), return the doc-comment tokens that form the run
/// IMMEDIATELY adjacent to the item — i.e. the run not followed by a
/// blank line, a regular comment, or any unexpected token before the
/// stream ends.
fn extract_doc_run(trivia: &[SyntaxToken]) -> Vec<SyntaxToken> {
    let mut current_run: Vec<SyntaxToken> = Vec::new();
    for t in trivia {
        match t.kind() {
            SyntaxKind::DOC_COMMENT => current_run.push(t.clone()),
            SyntaxKind::WHITESPACE => {
                let newlines = t.text().matches('\n').count();
                if newlines >= 2 {
                    // blank-line break: detach previous comments
                    current_run.clear();
                }
                // single newline: keep run together
            }
            _ => {
                // line/block comment or anything else: detach
                current_run.clear();
            }
        }
    }
    current_run
}

/// Render a stack of `///`-tokens as the raw markdown body (one line
/// per token, leading `///` and one optional space stripped).
fn doc_block_text(tokens: &[SyntaxToken]) -> String {
    let mut out = String::new();
    for t in tokens {
        let line = strip_doc_marker(t.text(), "///");
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

fn strip_doc_marker<'a>(line: &'a str, marker: &str) -> &'a str {
    let after = line.trim_start_matches(marker);
    after.strip_prefix(' ').unwrap_or(after)
}

/// Extract the file-level `//!` bang-doc block. The lexer doesn't
/// have a dedicated `INNER_DOC_COMMENT` kind (`//!` is lexed as
/// LINE_COMMENT), so we re-scan the leading trivia of the first
/// child for LINE_COMMENT tokens whose text starts with `//!`.
fn extract_file_doc(file: &File) -> (String, String) {
    let mut bang_lines: Vec<String> = Vec::new();
    for el in file.0.children_with_tokens() {
        match el {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::WHITESPACE => continue,
                SyntaxKind::LINE_COMMENT if t.text().starts_with("//!") => {
                    bang_lines.push(strip_doc_marker(t.text(), "//!").to_string());
                }
                _ => break,
            },
            NodeOrToken::Node(_) => break,
        }
    }
    let body = bang_lines.join("\n");
    let synopsis = synopsis_of(&body);
    (synopsis, body)
}

// ---------------------------------------------------------------------------
// Per-item builders
// ---------------------------------------------------------------------------

fn doc_from_fn(pkg: &Package, node: &SyntaxNode, raw: &str) -> Option<DocItem> {
    let f = find_fn_by_span(pkg, node)?;
    let f = &pkg.fns[f];
    if f.name.is_empty() {
        return None;
    }
    let sig = render_fn_signature(pkg, f);
    let (synopsis, body, examples, since) = parse_doc_body(raw);
    Some(DocItem {
        name: f.name.clone(),
        kind: DocItemKind::Fn,
        visibility: if f.is_pub {
            DocVisibility::Public
        } else {
            DocVisibility::Private
        },
        signature: ItemSignature {
            html: linkify_signature(&sig, pkg),
            plain: sig,
        },
        synopsis,
        body,
        examples,
        since,
        used_by: Vec::new(),
        anchor: format!("fn.{}", f.name),
    })
}

fn doc_from_struct(pkg: &Package, node: &SyntaxNode, raw: &str) -> Option<DocItem> {
    let id = pkg.structs.iter().find_map(|(id, s)| {
        if span_eq(&s.span, node) {
            Some(id)
        } else {
            None
        }
    })?;
    let s = &pkg.structs[id];
    let mut sig = String::new();
    if s.is_pub {
        sig.push_str("pub ");
    }
    sig.push_str("struct ");
    sig.push_str(&s.name);
    if !s.generics.is_empty() {
        sig.push('[');
        sig.push_str(&s.generics.join(", "));
        sig.push(']');
    }
    if !s.fields.is_empty() {
        sig.push_str(" {\n");
        for f in &s.fields {
            sig.push_str("    ");
            sig.push_str(&f.name);
            sig.push_str(": ");
            sig.push_str(&render_type(pkg, f.ty));
            sig.push_str(",\n");
        }
        sig.push('}');
    } else {
        sig.push_str(" {}");
    }
    let (synopsis, body, examples, since) = parse_doc_body(raw);
    Some(DocItem {
        name: s.name.clone(),
        kind: DocItemKind::Struct,
        visibility: vis_of(s.is_pub),
        signature: ItemSignature {
            html: linkify_signature(&sig, pkg),
            plain: sig,
        },
        synopsis,
        body,
        examples,
        since,
        used_by: Vec::new(),
        anchor: format!("struct.{}", s.name),
    })
}

fn doc_from_enum(pkg: &Package, node: &SyntaxNode, raw: &str) -> Option<DocItem> {
    let id = pkg.enums.iter().find_map(|(id, e)| {
        if span_eq(&e.span, node) {
            Some(id)
        } else {
            None
        }
    })?;
    let e = &pkg.enums[id];
    let mut sig = String::new();
    if e.is_pub {
        sig.push_str("pub ");
    }
    sig.push_str("enum ");
    sig.push_str(&e.name);
    if !e.generics.is_empty() {
        sig.push('[');
        sig.push_str(&e.generics.join(", "));
        sig.push(']');
    }
    sig.push_str(" {\n");
    for v in &e.variants {
        sig.push_str("    ");
        sig.push_str(&v.name);
        if !v.payload.is_empty() {
            sig.push('(');
            let pl: Vec<String> = v.payload.iter().map(|t| render_type(pkg, *t)).collect();
            sig.push_str(&pl.join(", "));
            sig.push(')');
        }
        sig.push_str(",\n");
    }
    sig.push('}');
    let (synopsis, body, examples, since) = parse_doc_body(raw);
    Some(DocItem {
        name: e.name.clone(),
        kind: DocItemKind::Enum,
        visibility: vis_of(e.is_pub),
        signature: ItemSignature {
            html: linkify_signature(&sig, pkg),
            plain: sig,
        },
        synopsis,
        body,
        examples,
        since,
        used_by: Vec::new(),
        anchor: format!("enum.{}", e.name),
    })
}

fn doc_from_type_alias(pkg: &Package, node: &SyntaxNode, raw: &str) -> Option<DocItem> {
    let id = pkg.type_aliases.iter().find_map(|(id, t)| {
        if span_eq(&t.span, node) {
            Some(id)
        } else {
            None
        }
    })?;
    let t = &pkg.type_aliases[id];
    let mut sig = String::new();
    if t.is_pub {
        sig.push_str("pub ");
    }
    sig.push_str("type ");
    sig.push_str(&t.name);
    if !t.generics.is_empty() {
        sig.push('[');
        sig.push_str(&t.generics.join(", "));
        sig.push(']');
    }
    sig.push_str(" = ");
    sig.push_str(&render_type(pkg, t.ty));
    let (synopsis, body, examples, since) = parse_doc_body(raw);
    Some(DocItem {
        name: t.name.clone(),
        kind: DocItemKind::TypeAlias,
        visibility: vis_of(t.is_pub),
        signature: ItemSignature {
            html: linkify_signature(&sig, pkg),
            plain: sig,
        },
        synopsis,
        body,
        examples,
        since,
        used_by: Vec::new(),
        anchor: format!("type.{}", t.name),
    })
}

fn doc_from_agent(pkg: &Package, node: &SyntaxNode, raw: &str) -> Option<DocItem> {
    let id = pkg.agents.iter().find_map(|(id, a)| {
        if span_eq(&a.span, node) {
            Some(id)
        } else {
            None
        }
    })?;
    let a = &pkg.agents[id];
    let mut sig = String::new();
    sig.push_str("agent ");
    sig.push_str(&a.name);
    if !a.ctor_params.is_empty() {
        sig.push('(');
        sig.push_str(&a.ctor_params.join(", "));
        sig.push(')');
    }
    if !a.protocols.is_empty() {
        sig.push_str(": ");
        let ps: Vec<String> = a.protocols.iter().map(|t| render_type(pkg, *t)).collect();
        sig.push_str(&ps.join(" + "));
    }
    if !a.handlers.is_empty() {
        sig.push_str(" {\n");
        for h in &a.handlers {
            sig.push_str("    on ");
            sig.push_str(&h.message);
            sig.push('(');
            sig.push_str(&h.params.join(", "));
            sig.push_str(")\n");
        }
        sig.push('}');
    }
    let (synopsis, body, examples, since) = parse_doc_body(raw);
    Some(DocItem {
        name: a.name.clone(),
        kind: DocItemKind::Agent,
        // Agents are public-by-convention in v0.2.
        visibility: DocVisibility::Public,
        signature: ItemSignature {
            html: linkify_signature(&sig, pkg),
            plain: sig,
        },
        synopsis,
        body,
        examples,
        since,
        used_by: Vec::new(),
        anchor: format!("agent.{}", a.name),
    })
}

fn doc_from_protocol(pkg: &Package, node: &SyntaxNode, raw: &str) -> Option<DocItem> {
    let id = pkg.protocols.iter().find_map(|(id, p)| {
        if span_eq(&p.span, node) {
            Some(id)
        } else {
            None
        }
    })?;
    let p = &pkg.protocols[id];
    let mut sig = String::new();
    if p.is_pub {
        sig.push_str("pub ");
    }
    sig.push_str("protocol ");
    sig.push_str(&p.name);
    if let Some(comp) = &p.composition {
        sig.push_str(" = ");
        let cs: Vec<String> = comp.iter().map(|t| render_type(pkg, *t)).collect();
        sig.push_str(&cs.join(" + "));
    } else if !p.messages.is_empty() {
        sig.push_str(" {\n");
        for m in &p.messages {
            sig.push_str("    ");
            sig.push_str(&m.name);
            sig.push('(');
            let ps: Vec<String> = m
                .params
                .iter()
                .map(|pa| {
                    let ty = pa
                        .ty
                        .map(|t| render_type(pkg, t))
                        .unwrap_or_else(|| "_".to_string());
                    format!("{}: {}", pa.name, ty)
                })
                .collect();
            sig.push_str(&ps.join(", "));
            sig.push(')');
            if let Some(r) = m.reply {
                sig.push_str(" -> ");
                sig.push_str(&render_type(pkg, r));
            }
            sig.push('\n');
        }
        sig.push('}');
    }
    let (synopsis, body, examples, since) = parse_doc_body(raw);
    Some(DocItem {
        name: p.name.clone(),
        kind: DocItemKind::Protocol,
        visibility: vis_of(p.is_pub),
        signature: ItemSignature {
            html: linkify_signature(&sig, pkg),
            plain: sig,
        },
        synopsis,
        body,
        examples,
        since,
        used_by: Vec::new(),
        anchor: format!("protocol.{}", p.name),
    })
}

fn doc_from_supervisor(pkg: &Package, node: &SyntaxNode, raw: &str) -> Option<DocItem> {
    let id = pkg.supervisors.iter().find_map(|(id, s)| {
        if span_eq(&s.span, node) {
            Some(id)
        } else {
            None
        }
    })?;
    let s = &pkg.supervisors[id];
    let mut sig = String::new();
    sig.push_str("supervisor ");
    sig.push_str(&s.name);
    sig.push_str(" strategy ");
    sig.push_str(&s.strategy);
    if !s.children.is_empty() {
        sig.push_str(" {\n");
        for (n, _) in &s.children {
            sig.push_str("    child ");
            sig.push_str(n);
            sig.push('\n');
        }
        sig.push('}');
    }
    let (synopsis, body, examples, since) = parse_doc_body(raw);
    Some(DocItem {
        name: s.name.clone(),
        kind: DocItemKind::Supervisor,
        visibility: DocVisibility::Public,
        signature: ItemSignature {
            html: linkify_signature(&sig, pkg),
            plain: sig,
        },
        synopsis,
        body,
        examples,
        since,
        used_by: Vec::new(),
        anchor: format!("supervisor.{}", s.name),
    })
}

fn doc_from_trait(pkg: &Package, node: &SyntaxNode, raw: &str) -> Option<DocItem> {
    let t = pkg.items.iter().find_map(|(_, item)| match item {
        Item::Trait(t) if span_eq(&t.span, node) => Some(t),
        _ => None,
    })?;
    let mut sig = String::new();
    if t.is_pub {
        sig.push_str("pub ");
    }
    sig.push_str("trait ");
    sig.push_str(&t.name);
    if !t.generics.is_empty() {
        sig.push('[');
        sig.push_str(&t.generics.join(", "));
        sig.push(']');
    }
    if !t.methods.is_empty() {
        sig.push_str(" {\n");
        for fid in &t.methods {
            let f = &pkg.fns[*fid];
            sig.push_str("    ");
            sig.push_str(&render_fn_signature(pkg, f));
            sig.push('\n');
        }
        sig.push('}');
    }
    let (synopsis, body, examples, since) = parse_doc_body(raw);
    Some(DocItem {
        name: t.name.clone(),
        kind: DocItemKind::Trait,
        visibility: vis_of(t.is_pub),
        signature: ItemSignature {
            html: linkify_signature(&sig, pkg),
            plain: sig,
        },
        synopsis,
        body,
        examples,
        since,
        used_by: Vec::new(),
        anchor: format!("trait.{}", t.name),
    })
}

fn doc_from_mod(node: &SyntaxNode, raw: &str) -> Option<DocModule> {
    let path: Vec<String> = node
        .children()
        .find_map(sdust_ast::Path::cast)
        .map(|p| {
            p.0.children()
                .filter_map(sdust_ast::PathSegment::cast)
                .map(|s| s.0.first_token().map(|t| t.text().to_string()).unwrap_or_default())
                .collect()
        })
        .unwrap_or_default();
    if path.is_empty() {
        return None;
    }
    Some(DocModule {
        synopsis: synopsis_of(raw),
        path,
    })
}

// ---------------------------------------------------------------------------
// HIR helpers
// ---------------------------------------------------------------------------

fn vis_of(is_pub: bool) -> DocVisibility {
    if is_pub {
        DocVisibility::Public
    } else {
        DocVisibility::Private
    }
}

fn span_eq(span: &SourceSpan, node: &SyntaxNode) -> bool {
    let r = node.text_range();
    let start: u32 = r.start().into();
    let end: u32 = r.end().into();
    span.start == start && span.end == end
}

fn find_fn_by_span(pkg: &Package, node: &SyntaxNode) -> Option<FnId> {
    pkg.fns.iter().find_map(|(id, f)| {
        if span_eq(&f.span, node) {
            Some(id)
        } else {
            None
        }
    })
}

fn file_package_name(file: &File) -> Option<String> {
    let pd = file
        .0
        .children()
        .find(|c| c.kind() == SyntaxKind::PACKAGE_DECL)?;
    let path = pd.children().find_map(sdust_ast::Path::cast)?;
    let segs: Vec<String> = path
        .0
        .children()
        .filter_map(sdust_ast::PathSegment::cast)
        .map(|s| {
            s.0.first_token()
                .map(|t| t.text().to_string())
                .unwrap_or_default()
        })
        .collect();
    if segs.is_empty() {
        None
    } else {
        Some(segs.join("."))
    }
}

fn render_fn_signature(pkg: &Package, f: &HirFn) -> String {
    let mut s = String::new();
    if f.is_pub {
        s.push_str("pub ");
    }
    if f.is_unsafe {
        s.push_str("unsafe ");
    }
    s.push_str("fn ");
    s.push_str(&f.name);
    if !f.generics.is_empty() {
        s.push('[');
        s.push_str(&f.generics.join(", "));
        s.push(']');
    }
    s.push('(');
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            let ty = p
                .ty
                .map(|t| render_type(pkg, t))
                .unwrap_or_else(|| "_".to_string());
            format!("{}: {}", p.name, ty)
        })
        .collect();
    s.push_str(&params.join(", "));
    s.push(')');
    if let Some(r) = f.ret {
        s.push_str(" -> ");
        s.push_str(&render_type(pkg, r));
    }
    if !f.effects.is_empty() {
        s.push_str(" effect ");
        s.push_str(&f.effects.join(", "));
    }
    s
}

pub(crate) fn render_type(pkg: &Package, ty: TypeId) -> String {
    match &pkg.types[ty] {
        HirType::Path { segments, generics } => {
            let mut s = segments.join(".");
            if !generics.is_empty() {
                s.push('[');
                let gs: Vec<String> = generics.iter().map(|g| render_type(pkg, *g)).collect();
                s.push_str(&gs.join(", "));
                s.push(']');
            }
            s
        }
        HirType::Borrow { mutable, inner } => {
            let mut s = String::from("&");
            if *mutable {
                s.push_str("mut ");
            }
            s.push_str(&render_type(pkg, *inner));
            s
        }
        HirType::Tuple(els) => {
            let parts: Vec<String> = els.iter().map(|t| render_type(pkg, *t)).collect();
            format!("({})", parts.join(", "))
        }
        HirType::Array { elem, .. } => format!("[{}]", render_type(pkg, *elem)),
        HirType::Fn { params, ret } => {
            let ps: Vec<String> = params.iter().map(|t| render_type(pkg, *t)).collect();
            let mut s = format!("fn({})", ps.join(", "));
            if let Some(r) = ret {
                s.push_str(" -> ");
                s.push_str(&render_type(pkg, *r));
            }
            s
        }
        HirType::Result { ok, err } => {
            format!("{}!{}", render_type(pkg, *ok), render_type(pkg, *err))
        }
        HirType::Union(parts) => {
            let ps: Vec<String> = parts.iter().map(|t| render_type(pkg, *t)).collect();
            ps.join(" | ")
        }
        HirType::Dyn { trait_name } => format!("dyn {}", trait_name),
        HirType::Unit => "Unit".to_string(),
        HirType::Unknown => "_".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Doc body parsing
// ---------------------------------------------------------------------------

/// Split the doc body into (synopsis, body, examples, since).
///
/// - Synopsis = first sentence (ends at `. ` or end of first line).
/// - Examples = fenced code blocks tagged `sd` or `stardust`. The raw
///   markdown body is left untouched (the renderer can re-extract or
///   render in place).
/// - Since = the text following a `# Since` heading on its own line.
pub fn parse_doc_body(raw: &str) -> (String, String, Vec<DocExample>, Option<String>) {
    if raw.trim().is_empty() {
        return (String::new(), String::new(), Vec::new(), None);
    }
    let synopsis = synopsis_of(raw);
    let since = extract_since(raw);
    let examples = extract_examples(raw);
    (synopsis, raw.to_string(), examples, since)
}

/// First sentence of `body`. Stops at the first `.` followed by
/// whitespace or end-of-input, or at the first blank line, whichever
/// comes first. Returns an empty string if `body` is empty.
pub fn synopsis_of(body: &str) -> String {
    let body = body.trim_start();
    if body.is_empty() {
        return String::new();
    }
    // First, cap at the first blank-line paragraph break.
    let para_end = body.find("\n\n").unwrap_or(body.len());
    let para = &body[..para_end];

    // Find `.` followed by whitespace or end.
    let bytes = para.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            let next = bytes.get(i + 1).copied();
            match next {
                None => return para[..=i].trim().to_string(),
                Some(c) if c.is_ascii_whitespace() => {
                    return para[..=i].trim().to_string();
                }
                _ => {}
            }
        }
        i += 1;
    }
    para.trim().to_string()
}

fn extract_since(body: &str) -> Option<String> {
    let mut lines = body.lines();
    while let Some(line) = lines.next() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("# Since") {
            // Same-line form: `# Since 0.2.0` or `# Since: 0.2.0`.
            let val = rest.trim().trim_start_matches(':').trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
            // Next non-empty line: `# Since\n0.2.0`.
            for next in lines.by_ref() {
                let n = next.trim();
                if !n.is_empty() {
                    return Some(n.to_string());
                }
            }
        }
    }
    None
}

fn extract_examples(body: &str) -> Vec<DocExample> {
    use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag};
    let parser = Parser::new(body);
    let mut examples = Vec::new();
    let mut current: Option<(String, String)> = None; // (language, code)
    for ev in parser {
        match ev {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) => {
                let lang = lang.to_string();
                if lang == "sd" || lang == "stardust" {
                    current = Some((lang, String::new()));
                } else {
                    current = None;
                }
            }
            Event::Text(t) => {
                if let Some((_, code)) = current.as_mut() {
                    code.push_str(&t);
                }
            }
            Event::End(_) => {
                if let Some((lang, code)) = current.take() {
                    examples.push(DocExample {
                        code,
                        language: lang,
                    });
                }
            }
            _ => {}
        }
    }
    examples
}

// ---------------------------------------------------------------------------
// Linkification
// ---------------------------------------------------------------------------

/// Replace bare type names in a plain signature with `<a href="#anchor">Name</a>`
/// when the name corresponds to a documented item in this package.
/// Conservative: only matches whole-word identifiers, and only on a
/// pre-built name → anchor table.
fn linkify_signature(sig: &str, pkg: &Package) -> String {
    let mut names: Vec<(String, String)> = Vec::new();
    for (_, s) in pkg.structs.iter() {
        names.push((s.name.clone(), format!("struct.{}", s.name)));
    }
    for (_, e) in pkg.enums.iter() {
        names.push((e.name.clone(), format!("enum.{}", e.name)));
    }
    for (_, it) in pkg.items.iter() {
        if let Item::Trait(t) = it {
            names.push((t.name.clone(), format!("trait.{}", t.name)));
        }
    }
    for (_, a) in pkg.agents.iter() {
        names.push((a.name.clone(), format!("agent.{}", a.name)));
    }
    for (_, p) in pkg.protocols.iter() {
        names.push((p.name.clone(), format!("protocol.{}", p.name)));
    }
    for (_, ta) in pkg.type_aliases.iter() {
        names.push((ta.name.clone(), format!("type.{}", ta.name)));
    }
    // Longest first so substring conflicts don't reorder.
    names.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    let mut html = html_escape(sig);
    for (name, anchor) in &names {
        if name.is_empty() {
            continue;
        }
        let replacement = format!("<a href=\"#{}\">{}</a>", anchor, name);
        html = replace_word(&html, name, &replacement);
    }
    html
}

/// Render `[Name]` doc-comment links to anchor links + escape inline
/// `[`/`]`. Used by the markdown and HTML renderers.
pub fn linkify_doc_text(text: &str, pkg_items: &[&DocItem]) -> String {
    let mut out = String::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            // try to find closing `]` on the same line
            if let Some(off) = text[i + 1..].find(']') {
                let end = i + 1 + off;
                let name = &text[i + 1..end];
                if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    if let Some(it) = pkg_items.iter().find(|d| d.name == name) {
                        out.push_str(&format!("[{}](#{})", name, it.anchor));
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn replace_word(haystack: &str, needle: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(haystack.len());
    let bytes = haystack.as_bytes();
    let n = needle.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + n.len() <= bytes.len() && &bytes[i..i + n.len()] == n {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after_ok = i + n.len() == bytes.len() || !is_ident_byte(bytes[i + n.len()]);
            if before_ok && after_ok {
                out.push_str(replacement);
                i += n.len();
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

pub(crate) fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Backlinks
// ---------------------------------------------------------------------------

/// Walk every expression body and credit every `Path` whose final
/// segment matches a documented item. The owning fn's name is added
/// to that item's `used_by` list. Self-references are skipped.
pub fn compute_backlinks(pkg: &Package, doc: &mut DocPackage) {
    use std::collections::{HashMap, HashSet};
    let mut name_to_idx: HashMap<String, usize> = HashMap::new();
    for (i, it) in doc.items.iter().enumerate() {
        name_to_idx.insert(it.name.clone(), i);
    }
    let mut accum: Vec<HashSet<String>> = vec![HashSet::new(); doc.items.len()];

    for (_, f) in pkg.fns.iter() {
        if let Some(body) = f.body {
            visit_block(pkg, body, &f.name, &name_to_idx, &mut accum);
        }
    }
    for (_, a) in pkg.agents.iter() {
        for h in &a.handlers {
            let owner = format!("{}.{}", a.name, h.message);
            visit_block(pkg, h.body, &owner, &name_to_idx, &mut accum);
        }
    }

    for (i, set) in accum.into_iter().enumerate() {
        let mut v: Vec<String> = set.into_iter().collect();
        v.sort();
        doc.items[i].used_by = v;
    }
}

fn visit_block(
    pkg: &Package,
    block: BlockId,
    owner: &str,
    names: &std::collections::HashMap<String, usize>,
    accum: &mut [std::collections::HashSet<String>],
) {
    let b = &pkg.blocks[block];
    for s in &b.stmts {
        match s {
            HirStmt::Let { init, .. } => {
                if let Some(e) = init {
                    visit_expr(pkg, *e, owner, names, accum);
                }
            }
            HirStmt::Expr(e) => visit_expr(pkg, *e, owner, names, accum),
        }
    }
    if let Some(t) = b.tail {
        visit_expr(pkg, t, owner, names, accum);
    }
}

fn visit_expr(
    pkg: &Package,
    expr: ExprId,
    owner: &str,
    names: &std::collections::HashMap<String, usize>,
    accum: &mut [std::collections::HashSet<String>],
) {
    use HirExpr::*;
    let e = &pkg.exprs[expr];
    match e {
        Path(segs) | PathGeneric { segments: segs, .. } => {
            if let Some(last) = segs.last() {
                if let Some(idx) = names.get(last) {
                    accum[*idx].insert(owner.to_string());
                }
            }
        }
        Call { callee, args } => {
            visit_expr(pkg, *callee, owner, names, accum);
            for a in args {
                visit_expr(pkg, a.value, owner, names, accum);
            }
        }
        MethodCall { receiver, args, .. } => {
            visit_expr(pkg, *receiver, owner, names, accum);
            for a in args {
                visit_expr(pkg, a.value, owner, names, accum);
            }
        }
        Field { receiver, .. } => visit_expr(pkg, *receiver, owner, names, accum),
        Index { receiver, idx } => {
            visit_expr(pkg, *receiver, owner, names, accum);
            visit_expr(pkg, *idx, owner, names, accum);
        }
        Binary { lhs, rhs, .. } => {
            visit_expr(pkg, *lhs, owner, names, accum);
            visit_expr(pkg, *rhs, owner, names, accum);
        }
        Unary { rhs, .. } => visit_expr(pkg, *rhs, owner, names, accum),
        If { cond, then, else_ } => {
            visit_expr(pkg, *cond, owner, names, accum);
            visit_block(pkg, *then, owner, names, accum);
            if let Some(el) = else_ {
                visit_expr(pkg, *el, owner, names, accum);
            }
        }
        Match { scrutinee, arms } => {
            visit_expr(pkg, *scrutinee, owner, names, accum);
            for arm in arms {
                visit_expr(pkg, arm.body, owner, names, accum);
            }
        }
        For { iter, body, .. } => {
            visit_expr(pkg, *iter, owner, names, accum);
            visit_block(pkg, *body, owner, names, accum);
        }
        While { cond, body } => {
            visit_expr(pkg, *cond, owner, names, accum);
            visit_block(pkg, *body, owner, names, accum);
        }
        Loop { body } => visit_block(pkg, *body, owner, names, accum),
        Return(Some(e)) => visit_expr(pkg, *e, owner, names, accum),
        Block(b) => visit_block(pkg, *b, owner, names, accum),
        Tuple(xs) | Array(xs) => {
            for x in xs {
                visit_expr(pkg, *x, owner, names, accum);
            }
        }
        Struct { fields, .. } => {
            for (_, v) in fields {
                visit_expr(pkg, *v, owner, names, accum);
            }
        }
        Map(entries) => {
            for (k, v) in entries {
                visit_expr(pkg, *k, owner, names, accum);
                visit_expr(pkg, *v, owner, names, accum);
            }
        }
        Send { target, args, .. } | Ask { target, args, .. } => {
            visit_expr(pkg, *target, owner, names, accum);
            for a in args {
                visit_expr(pkg, a.value, owner, names, accum);
            }
        }
        Deadline { inner, dur } => {
            visit_expr(pkg, *inner, owner, names, accum);
            visit_expr(pkg, *dur, owner, names, accum);
        }
        Question(e) | Move(e) | Detach(e) | Join(e) | Run(e) => {
            visit_expr(pkg, *e, owner, names, accum)
        }
        Borrow { inner, .. } => visit_expr(pkg, *inner, owner, names, accum),
        Spawn { inner, .. } => visit_expr(pkg, *inner, owner, names, accum),
        Cast { lhs, .. } => visit_expr(pkg, *lhs, owner, names, accum),
        Arena { body, .. } => visit_expr(pkg, *body, owner, names, accum),
        TaskScope { deadline, body } => {
            if let Some(d) = deadline {
                visit_expr(pkg, *d, owner, names, accum);
            }
            visit_block(pkg, *body, owner, names, accum);
        }
        Budget { entries, body } => {
            for (_, e) in entries {
                visit_expr(pkg, *e, owner, names, accum);
            }
            visit_expr(pkg, *body, owner, names, accum);
        }
        Sandbox { entries, body, .. } => {
            for (_, e) in entries {
                visit_expr(pkg, *e, owner, names, accum);
            }
            visit_block(pkg, *body, owner, names, accum);
        }
        Lambda { body, .. } => visit_block(pkg, *body, owner, names, accum),
        Unsafe(b) => visit_block(pkg, *b, owner, names, accum),
        IfLet {
            scrutinee,
            then,
            else_,
            ..
        } => {
            visit_expr(pkg, *scrutinee, owner, names, accum);
            visit_block(pkg, *then, owner, names, accum);
            if let Some(el) = else_ {
                visit_expr(pkg, *el, owner, names, accum);
            }
        }
        Literal(_) | HtmlTemplate(_) | Return(None) | Error => {}
    }
}
