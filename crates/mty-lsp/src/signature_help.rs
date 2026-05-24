//! Signature help — `textDocument/signatureHelp`.
//!
//! When the cursor sits inside a call expression `foo(a, b|, c)`, we
//! show the resolved signature of `foo` with the active parameter
//! highlighted. The active parameter is computed by counting unescaped
//! commas between the opening `(` of the call and the cursor.
//!
//! Resolution strategy:
//! - Walk up the CST from the cursor token to find the smallest
//!   enclosing `CALL_EXPR` or `METHOD_CALL_EXPR`.
//! - For `CALL_EXPR`, extract the callee path (first IDENT-leading
//!   token sequence) and look it up in `DefMap::by_name`. If it's a
//!   fn, render its signature.
//! - For `METHOD_CALL_EXPR`, extract the method name and present a
//!   conservative "method `<name>` of <receiver-type>" signature based
//!   on the receiver's inferred type (best-effort).
//!
//! v0.5 caveats: doesn't yet handle nested generic args, doesn't yet
//! emit per-overload information for trait-method resolution (multiple
//! `impl Foo` providing the same `bar()` — we list all candidates).

use crate::docs::DocAnalysis;
use rowan::TextSize;
use mty_syntax::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};
use mty_types::{pretty_ty, DefRef, FnDef};
use tower_lsp::lsp_types::{
    ParameterInformation, ParameterLabel, Position, SignatureHelp, SignatureInformation,
};

pub fn signature_help(doc: &DocAnalysis, pos: Position) -> Option<SignatureHelp> {
    let offset = doc
        .line_index
        .position_to_offset(&doc.source, pos.line, pos.character);
    let root = SyntaxNode::new_root(doc.parsed.green.clone());
    let call = enclosing_call(&root, offset)?;
    let active_param = active_param_index(&call, offset);

    match call.kind() {
        SyntaxKind::CALL_EXPR => signature_for_call(&call, doc, active_param),
        SyntaxKind::METHOD_CALL_EXPR => signature_for_method_call(&call, doc, active_param),
        _ => None,
    }
}

fn signature_for_call(
    call: &SyntaxNode,
    doc: &DocAnalysis,
    active_param: u32,
) -> Option<SignatureHelp> {
    let name = call_callee_name(call)?;
    let def = doc.typed.def_map.by_name.get(&name)?;
    match def {
        DefRef::Fn(id) => {
            let f = doc.typed.def_map.fn_def(*id)?;
            Some(render_fn_signature(
                f,
                &doc.typed.ty_arena,
                &doc.typed.def_map,
                active_param,
            ))
        }
        DefRef::Variant(adt_id, vidx) => {
            // Enum constructor call: render the variant payload types.
            let a = doc.typed.def_map.adt(*adt_id)?;
            let v = a.variants.get(*vidx)?;
            let parts: Vec<String> = v
                .fields
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let nm = f.name.clone().unwrap_or_else(|| format!("_{}", i));
                    let ty = pretty_ty(f.ty, &doc.typed.ty_arena, None, Some(&doc.typed.def_map));
                    format!("{}: {}", nm, ty)
                })
                .collect();
            let label = format!("{}.{}({})", a.name, v.name, parts.join(", "));
            Some(SignatureHelp {
                signatures: vec![sig_info(label, parts.len(), active_param)],
                active_signature: Some(0),
                active_parameter: Some(active_param),
            })
        }
        _ => None,
    }
}

fn signature_for_method_call(
    call: &SyntaxNode,
    doc: &DocAnalysis,
    active_param: u32,
) -> Option<SignatureHelp> {
    let method = method_name(call)?;
    // Best-effort: look up the method in `builtin_methods` for the
    // documented arity. Real trait-resolution is deferred.
    if let Some(m) = doc.typed.def_map.builtin_methods.get(&method) {
        let arity = m.arity.unwrap_or(0);
        let params: Vec<String> = (0..arity).map(|i| format!("arg{}", i)).collect();
        let label = format!(".{}({})", method, params.join(", "));
        return Some(SignatureHelp {
            signatures: vec![sig_info(label, arity, active_param)],
            active_signature: Some(0),
            active_parameter: Some(active_param),
        });
    }
    // Fall back: search impl_methods on every ADT — list every match.
    let mut sigs: Vec<SignatureInformation> = Vec::new();
    for ((_adt, mname), fid) in doc.typed.def_map.impl_methods.iter() {
        if *mname == method {
            if let Some(f) = doc.typed.def_map.fn_def(*fid) {
                sigs.push(
                    render_fn_signature(f, &doc.typed.ty_arena, &doc.typed.def_map, active_param)
                        .signatures
                        .into_iter()
                        .next()
                        .unwrap(),
                );
            }
        }
    }
    if sigs.is_empty() {
        return None;
    }
    Some(SignatureHelp {
        signatures: sigs,
        active_signature: Some(0),
        active_parameter: Some(active_param),
    })
}

fn render_fn_signature(
    f: &FnDef,
    arena: &mty_types::TyArena,
    defs: &mty_types::DefMap,
    active_param: u32,
) -> SignatureHelp {
    let parts: Vec<String> = f
        .params
        .iter()
        .map(|(n, t)| format!("{}: {}", n, pretty_ty(*t, arena, None, Some(defs))))
        .collect();
    let ret = pretty_ty(f.ret, arena, None, Some(defs));
    let label = format!("fn {}({}) -> {}", f.name, parts.join(", "), ret);
    SignatureHelp {
        signatures: vec![sig_info(label, parts.len(), active_param)],
        active_signature: Some(0),
        active_parameter: Some(active_param),
    }
}

fn sig_info(label: String, arity: usize, _active: u32) -> SignatureInformation {
    // Build per-parameter labels using offsets into `label`. We just
    // use simple string params here for portability.
    let mut params: Vec<ParameterInformation> = Vec::new();
    for i in 0..arity {
        params.push(ParameterInformation {
            label: ParameterLabel::Simple(format!("p{}", i)),
            documentation: None,
        });
    }
    SignatureInformation {
        label,
        documentation: None,
        parameters: Some(params),
        active_parameter: None,
    }
}

fn enclosing_call(root: &SyntaxNode, offset: u32) -> Option<SyntaxNode> {
    let pos = TextSize::from(offset);
    let mut found: Option<SyntaxNode> = None;
    for n in root.descendants() {
        if !matches!(
            n.kind(),
            SyntaxKind::CALL_EXPR | SyntaxKind::METHOD_CALL_EXPR
        ) {
            continue;
        }
        // Cursor must be inside the parenthesized arg list of this
        // call. We accept the cursor sitting AT the end-byte of the arg
        // list (rowan's `contains` is exclusive on end, so use
        // `pos >= start && pos <= end`).
        let in_args = n
            .children()
            .find(|c| c.kind() == SyntaxKind::ARG_LIST)
            .map(|args| {
                let r = args.text_range();
                pos >= r.start() && pos <= r.end()
            })
            .unwrap_or(false);
        if !in_args {
            continue;
        }
        match &found {
            None => found = Some(n.clone()),
            Some(prev) if n.text_range().len() < prev.text_range().len() => found = Some(n.clone()),
            _ => {}
        }
    }
    found
}

fn active_param_index(call: &SyntaxNode, cursor: u32) -> u32 {
    // Count commas between the `(` of the ARG_LIST and `cursor`.
    let Some(args) = call.children().find(|c| c.kind() == SyntaxKind::ARG_LIST) else {
        return 0;
    };
    let cursor_pos = TextSize::from(cursor);
    let mut depth: i32 = 0;
    let mut commas: u32 = 0;
    for d in args.descendants_with_tokens() {
        match d {
            SyntaxElement::Token(t) => {
                let start: u32 = t.text_range().start().into();
                if t.text_range().start() >= cursor_pos {
                    break;
                }
                match t.kind() {
                    SyntaxKind::L_PAREN | SyntaxKind::L_BRACK | SyntaxKind::L_BRACE => depth += 1,
                    SyntaxKind::R_PAREN | SyntaxKind::R_BRACK | SyntaxKind::R_BRACE => depth -= 1,
                    SyntaxKind::COMMA if depth == 1 => commas += 1,
                    _ => {}
                }
                let _ = start;
            }
            SyntaxElement::Node(_) => {}
        }
    }
    commas
}

fn call_callee_name(call: &SyntaxNode) -> Option<String> {
    // CALL_EXPR has a callee expression node before the ARG_LIST. We
    // look for a leading PATH_EXPR / NAME_REF and take its last segment.
    let callee = call
        .children()
        .find(|c| !matches!(c.kind(), SyntaxKind::ARG_LIST))?;
    last_ident(&callee).map(|t| t.text().to_string())
}

fn method_name(call: &SyntaxNode) -> Option<String> {
    // METHOD_CALL_EXPR: receiver `.` method `(` args `)` — the IDENT
    // sandwiched between the DOT and the L_PAREN is the method name.
    let mut last_ident_tok: Option<SyntaxToken> = None;
    for d in call.children_with_tokens() {
        if let Some(t) = d.as_token() {
            if t.kind() == SyntaxKind::IDENT {
                last_ident_tok = Some(t.clone());
            }
            if t.kind() == SyntaxKind::L_PAREN {
                break;
            }
        }
    }
    last_ident_tok.map(|t| t.text().to_string())
}

fn last_ident(node: &SyntaxNode) -> Option<SyntaxToken> {
    let mut found: Option<SyntaxToken> = None;
    for d in node.descendants_with_tokens() {
        if let Some(t) = d.as_token() {
            if t.kind() == SyntaxKind::IDENT {
                found = Some(t.clone());
            }
        }
    }
    found
}
