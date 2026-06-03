//! Code actions — `textDocument/codeAction`.
//!
//! Two tracks of code actions ship today:
//!
//! 1. **Hand-written, scope-driven** — v0.5 fixes that ride the
//!    LSP's live type / scope information rather than re-running the
//!    fix engine. Coverage:
//!
//!    | code | fix                                                                 |
//!    |------|---------------------------------------------------------------------|
//!    | MT2021 unresolved value  | suggest top-3 in-scope names by edit distance |
//!    | MT2002 unresolved type   | suggest top-3 in-scope type names by edit distance |
//!    | MT3001 use-after-move    | suggest `.clone()` (best-effort; user confirms)    |
//!    | MT4001 effect undeclared | suggest adding the missing effect to the fn signature |
//!    | MT6001 unknown macro     | suggest top-3 in-scope macro names                |
//!
//! 2. **Envelope-driven (v0.34 T2)** — every Mighty diagnostic carries
//!    a `mty_diagnostics::fix::DiagnosticEnvelope` with one or more
//!    `FixAlternative`s. We surface them as LSP `CodeAction`s by
//!    parsing the embedded unified diff into [`TextEdit`]s (see
//!    [`crate::diff_apply`]).
//!
//!    Confidence filtering:
//!    - `>= preferred_threshold` (default 0.85) → `isPreferred = true`
//!    - `>= visible_threshold` (default 0.7) → emitted, not preferred
//!    - below `visible_threshold` → hidden from the lightbulb (still
//!      available via `mty fix --apply` on the CLI)
//!
//!    Thresholds are configurable per-server via
//!    [`CodeActionConfig::from_initialization_options`]. The defaults
//!    match the LSP setting `mighty.codeAction.confidenceThreshold`.
//!
//! Each action returns a single-text-edit [`WorkspaceEdit`] so the
//! editor can preview before applying. Multi-hunk diffs collapse to
//! one `WorkspaceEdit` with multiple edits, applied atomically.

use crate::diff_apply::unified_diff_to_text_edits;
use crate::docs::DocAnalysis;
use mty_diagnostics::fix::DiagnosticEnvelope;
use mty_diagnostics::{Diagnostic, ToEnvelope};
use mty_types::DefRef;
use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionResponse, DocumentChanges,
    NumberOrString, OneOf, OptionalVersionedTextDocumentIdentifier, Position, Range,
    TextDocumentEdit, TextEdit, Url, WorkspaceEdit,
};

/// Maximum edit distance for "did you mean" suggestions.
const SUGGEST_MAX_DISTANCE: usize = 2;

/// Default lower bound for an envelope fix to appear at all in the
/// lightbulb. Below this, the suggestion is still in the JSON envelope
/// (so `mty fix --apply` can use it) but the editor lightbulb hides it.
pub const DEFAULT_VISIBLE_THRESHOLD: f32 = 0.7;

/// Default lower bound for marking an action as the editor's
/// "preferred" fix (the one applied on `Quick Fix`). Above this,
/// `is_preferred = true`.
pub const DEFAULT_PREFERRED_THRESHOLD: f32 = 0.85;

/// Runtime-configurable thresholds, surfaced via the LSP
/// `mighty.codeAction.confidenceThreshold` setting.
#[derive(Debug, Clone, Copy)]
pub struct CodeActionConfig {
    pub visible_threshold: f32,
    pub preferred_threshold: f32,
}

impl Default for CodeActionConfig {
    fn default() -> Self {
        Self {
            visible_threshold: DEFAULT_VISIBLE_THRESHOLD,
            preferred_threshold: DEFAULT_PREFERRED_THRESHOLD,
        }
    }
}

impl CodeActionConfig {
    /// Read `mighty.codeAction.confidenceThreshold` from an
    /// `initializationOptions` JSON blob. Falls back to the default
    /// (0.7) when the key is missing or out of range.
    pub fn from_initialization_options(opts: &serde_json::Value) -> Self {
        let mut cfg = Self::default();
        let mighty = opts.get("mighty").unwrap_or(opts);
        if let Some(v) = mighty
            .get("codeAction")
            .and_then(|c| c.get("confidenceThreshold"))
            .and_then(|t| t.as_f64())
        {
            let v = v as f32;
            if (0.0..=1.0).contains(&v) {
                cfg.visible_threshold = v;
                // Keep the preferred threshold above the visible one.
                cfg.preferred_threshold = cfg.preferred_threshold.max(v);
            }
        }
        cfg
    }
}

/// v0.47 T5 — capability-negotiated `documentChanges` flag. Set when
/// the client advertises
/// `capabilities.workspace.workspaceEdit.documentChanges = true`; the
/// returned [`WorkspaceEdit`]s use the versioned `documentChanges`
/// shape instead of the legacy `changes` map.
#[derive(Debug, Clone, Copy, Default)]
pub struct WorkspaceEditCaps {
    pub document_changes: bool,
}

/// Top-level handler entry. `cursor_range` is the editor's current
/// selection (so we can scope the suggestions); `diagnostics` is the
/// list the client thinks applies at that range.
pub fn code_actions(
    uri: &Url,
    doc: &DocAnalysis,
    cursor_range: Range,
    diagnostics: &[tower_lsp::lsp_types::Diagnostic],
) -> CodeActionResponse {
    code_actions_with_config(
        uri,
        doc,
        cursor_range,
        diagnostics,
        CodeActionConfig::default(),
    )
}

/// Like [`code_actions`] but with caller-supplied confidence
/// thresholds. The server holds one `CodeActionConfig` per session,
/// updated from `initializationOptions`.
pub fn code_actions_with_config(
    uri: &Url,
    doc: &DocAnalysis,
    cursor_range: Range,
    diagnostics: &[tower_lsp::lsp_types::Diagnostic],
    cfg: CodeActionConfig,
) -> CodeActionResponse {
    code_actions_with_caps(uri, doc, cursor_range, diagnostics, cfg, WorkspaceEditCaps::default())
}

/// Like [`code_actions_with_config`] but also honours the v0.47 T5
/// `documentChanges` capability flag.
///
/// Internally builds with the legacy `changes` shape and post-processes
/// the result so existing helpers (`quickfix_with_edit`, etc.) stay
/// version-agnostic. When `caps.document_changes` is `false` this is
/// a no-op pass-through.
pub fn code_actions_with_caps(
    uri: &Url,
    doc: &DocAnalysis,
    cursor_range: Range,
    diagnostics: &[tower_lsp::lsp_types::Diagnostic],
    cfg: CodeActionConfig,
    caps: WorkspaceEditCaps,
) -> CodeActionResponse {
    let resp = code_actions_inner(uri, doc, cursor_range, diagnostics, cfg);
    if caps.document_changes {
        upgrade_response_to_document_changes(resp, doc.version)
    } else {
        resp
    }
}

/// Convert every `WorkspaceEdit.changes` carried inside a
/// [`CodeActionResponse`] to the `documentChanges` shape, stamping each
/// entry with the supplied buffer `version`.
fn upgrade_response_to_document_changes(
    resp: CodeActionResponse,
    version: i32,
) -> CodeActionResponse {
    resp.into_iter()
        .map(|item| match item {
            CodeActionOrCommand::CodeAction(mut ca) => {
                if let Some(edit) = ca.edit.take() {
                    ca.edit = Some(upgrade_workspace_edit(edit, version));
                }
                CodeActionOrCommand::CodeAction(ca)
            }
            other => other,
        })
        .collect()
}

fn upgrade_workspace_edit(edit: WorkspaceEdit, version: i32) -> WorkspaceEdit {
    let WorkspaceEdit {
        changes,
        document_changes,
        change_annotations,
    } = edit;
    if document_changes.is_some() {
        // Already documentChanges-shaped — pass through.
        return WorkspaceEdit {
            changes,
            document_changes,
            change_annotations,
        };
    }
    let Some(changes) = changes else {
        return WorkspaceEdit {
            changes: None,
            document_changes: None,
            change_annotations,
        };
    };
    let version_field = if version <= 0 { None } else { Some(version) };
    let edits: Vec<TextDocumentEdit> = changes
        .into_iter()
        .map(|(uri, te)| TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri,
                version: version_field,
            },
            edits: te.into_iter().map(OneOf::Left).collect(),
        })
        .collect();
    WorkspaceEdit {
        changes: None,
        document_changes: Some(DocumentChanges::Edits(edits)),
        change_annotations,
    }
}

fn code_actions_inner(
    uri: &Url,
    doc: &DocAnalysis,
    cursor_range: Range,
    diagnostics: &[tower_lsp::lsp_types::Diagnostic],
    cfg: CodeActionConfig,
) -> CodeActionResponse {
    let mut out: Vec<CodeActionOrCommand> = Vec::new();
    // Track titles we've already pushed so the envelope-driven path
    // doesn't duplicate the hand-written scope-aware suggestions.
    let mut seen_titles: std::collections::HashSet<String> = std::collections::HashSet::new();

    // -------- Pass 1: scope-aware hand-written fixes ----------
    for diag in diagnostics {
        let Some(NumberOrString::String(code)) = &diag.code else {
            continue;
        };
        let before = out.len();
        match code.as_str() {
            "MT2021" => actions_for_unresolved_value(uri, doc, diag, &mut out),
            "MT2002" => actions_for_unresolved_type(uri, doc, diag, &mut out),
            "MT3001" => actions_for_use_after_move(uri, doc, diag, &mut out),
            "MT4001" => actions_for_effect_undeclared(uri, doc, diag, &mut out),
            "MT6001" => actions_for_unknown_macro(uri, doc, diag, &mut out),
            _ => {}
        }
        for a in &out[before..] {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                seen_titles.insert(ca.title.clone());
            }
        }
    }
    // Empty-diag-list path: scope-aware fixes triggered by the
    // cursor position alone (no client round-trip yet).
    if diagnostics.is_empty() {
        for d in &doc.diagnostics {
            if diag_contains_cursor(d, doc, cursor_range) {
                let lsp_diag = crate::conv::diagnostic_to_lsp(d, &doc.line_index, &doc.source);
                let before = out.len();
                match d.code.as_str().as_str() {
                    "MT2021" => actions_for_unresolved_value(uri, doc, &lsp_diag, &mut out),
                    "MT2002" => actions_for_unresolved_type(uri, doc, &lsp_diag, &mut out),
                    "MT3001" => actions_for_use_after_move(uri, doc, &lsp_diag, &mut out),
                    "MT4001" => actions_for_effect_undeclared(uri, doc, &lsp_diag, &mut out),
                    "MT6001" => actions_for_unknown_macro(uri, doc, &lsp_diag, &mut out),
                    _ => {}
                }
                for a in &out[before..] {
                    if let CodeActionOrCommand::CodeAction(ca) = a {
                        seen_titles.insert(ca.title.clone());
                    }
                }
            }
        }
    }

    // -------- Pass 2: envelope-driven fixes (v0.34 T2) ----------
    // Build a map: { MTxxxx code → list of matching client diagnostics }.
    // For each Mighty Diagnostic in the document whose range overlaps
    // the cursor (or is in the client-passed diag list), pull its
    // envelope and surface every `FixAlternative` with confidence
    // ≥ visible_threshold as a CodeAction.
    let client_diag_codes: std::collections::HashSet<String> = diagnostics
        .iter()
        .filter_map(|d| match &d.code {
            Some(NumberOrString::String(s)) => Some(s.clone()),
            _ => None,
        })
        .collect();

    for d in &doc.diagnostics {
        let in_range = diag_contains_cursor(d, doc, cursor_range);
        let in_client_list = client_diag_codes.contains(&d.code.as_str());
        if !(in_range || in_client_list) {
            continue;
        }
        let env = d.to_envelope(uri.as_str(), &doc.source);
        let lsp_diag = crate::conv::diagnostic_to_lsp(d, &doc.line_index, &doc.source);
        emit_envelope_actions(
            uri,
            &env,
            &lsp_diag,
            &doc.source,
            cfg,
            &seen_titles,
            &mut out,
        );
    }

    out
}

/// Translate one envelope's fix alternatives into LSP `CodeAction`s.
/// Each `FixAlternative` becomes one action; alternatives below the
/// visible threshold are skipped. Titles already present in
/// `seen_titles` (from the hand-written pass) are skipped to avoid
/// duplicates.
fn emit_envelope_actions(
    uri: &Url,
    env: &DiagnosticEnvelope,
    lsp_diag: &tower_lsp::lsp_types::Diagnostic,
    source: &str,
    cfg: CodeActionConfig,
    seen_titles: &std::collections::HashSet<String>,
    out: &mut Vec<CodeActionOrCommand>,
) {
    let Some(fix) = env.fix.as_ref() else {
        return;
    };
    for alt in &fix.alternatives {
        if alt.confidence < cfg.visible_threshold {
            continue;
        }
        let title = format!("Apply: {}", alt.label);
        if seen_titles.contains(&title) {
            continue;
        }
        let edits = unified_diff_to_text_edits(&alt.diff, source);
        if edits.is_empty() {
            // The diff didn't line up with the source (probably an
            // off-by-one between fix engine + LSP doc version). Drop
            // the alternative rather than offer a broken edit.
            continue;
        }
        let kind = code_action_kind_for(&fix.kind);
        let is_preferred = alt.confidence >= cfg.preferred_threshold;
        out.push(envelope_action(
            uri,
            lsp_diag.clone(),
            edits,
            title,
            kind,
            is_preferred,
        ));
    }
}

/// Map a `FixKind` discriminator (`"untaint"`, `"rename_to_match_decl"`,
/// etc.) to the LSP `CodeActionKind` the editor should use. Most kinds
/// map to plain `quickfix`; rename / refactor-flavored fixes get a
/// `refactor.rewrite` kind so editors that show "Refactor" menus
/// separately surface them in the right place.
fn code_action_kind_for(fix_kind: &str) -> CodeActionKind {
    match fix_kind {
        // Renames + structural refactors live under `refactor.rewrite`.
        "rename_to_match_decl" | "add_struct_field" | "add_match_arm" => {
            CodeActionKind::REFACTOR_REWRITE
        }
        // Everything else is a quickfix.
        _ => CodeActionKind::QUICKFIX,
    }
}

fn envelope_action(
    uri: &Url,
    diag: tower_lsp::lsp_types::Diagnostic,
    edits: Vec<TextEdit>,
    title: String,
    kind: CodeActionKind,
    is_preferred: bool,
) -> CodeActionOrCommand {
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri.clone(), edits);
    CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(kind),
        diagnostics: Some(vec![diag]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(is_preferred),
        disabled: None,
        data: None,
    })
}

/// Surface every CodeActionKind T2 might emit so the server can
/// advertise them in `initialize` capabilities. Editors that filter
/// the `only` field can then ask for `refactor.rewrite` and get the
/// rename-flavored fixes back without the quick-fix lightbulb.
pub fn supported_code_action_kinds() -> Vec<CodeActionKind> {
    vec![
        CodeActionKind::QUICKFIX,
        CodeActionKind::REFACTOR_REWRITE,
        // Source-action kind reserved for v0.35 bulk-apply
        // ("Fix all Mighty problems"). Advertising it now means
        // editor toolbars can already include the entry.
        CodeActionKind::new("source.fixAll.mighty"),
    ]
}

/// Bulk apply identifier. Editors that bind to a fixed string can
/// trigger Mighty's "Fix all" by sending a `textDocument/codeAction`
/// with `context.only = ["source.fixAll.mighty"]`. The handler returns
/// a single [`CodeAction`] whose `WorkspaceEdit` carries every
/// preferred-confidence fix for the document, applied in source order
/// (highest line first, so earlier-in-file anchors stay valid).
pub const SOURCE_FIX_ALL_MIGHTY: &str = "source.fixAll.mighty";

/// v0.35 T3 — Build the single bulk-apply CodeAction for
/// `source.fixAll.mighty`. Returns `None` when the document has no
/// applicable preferred fixes.
///
/// "Preferred" = `confidence >= cfg.preferred_threshold` (default 0.85).
/// For each qualifying diagnostic we pick the highest-confidence
/// alternative whose diff applies cleanly against the current source.
/// Multiple alternatives at the same span are NOT applied; the action
/// chooses one per diagnostic, matching the CLI semantics.
///
/// Conflict resolution: the resulting `TextEdit`s share one
/// `WorkspaceEdit`. LSP guarantees `WorkspaceEdit` text edits are
/// applied highest-offset first, so our ordering (sorted descending
/// by line) is preserved end-to-end.
pub fn fix_all_mighty_action(
    uri: &Url,
    doc: &DocAnalysis,
    cfg: CodeActionConfig,
) -> Option<CodeAction> {
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut diags_touched: Vec<tower_lsp::lsp_types::Diagnostic> = Vec::new();

    // Walk every diagnostic in the document. For each one with a
    // preferred fix, splice its highest-confidence applicable diff
    // into the edit list.
    //
    // We collect (line, edit) pairs first so we can sort
    // highest-line-first before merging — LSP applies WorkspaceEdit
    // edits in descending offset order, so this keeps the source
    // splice anchors valid.
    let mut staged: Vec<(u32, TextEdit, tower_lsp::lsp_types::Diagnostic)> = Vec::new();

    for d in &doc.diagnostics {
        let env = d.to_envelope(uri.as_str(), &doc.source);
        let Some(fix) = env.fix.as_ref() else {
            continue;
        };

        // Pick the best alt that's preferred AND applies cleanly.
        let mut alts: Vec<&mty_diagnostics::fix::FixAlternative> = fix
            .alternatives
            .iter()
            .filter(|a| a.confidence >= cfg.preferred_threshold)
            .collect();
        alts.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for alt in alts {
            let new_edits = unified_diff_to_text_edits(&alt.diff, &doc.source);
            if new_edits.is_empty() {
                continue;
            }
            let lsp_diag = crate::conv::diagnostic_to_lsp(d, &doc.line_index, &doc.source);
            for e in new_edits {
                staged.push((env.span.line, e, lsp_diag.clone()));
            }
            break; // one alternative per diagnostic
        }
    }

    if staged.is_empty() {
        return None;
    }

    // Highest source line first.
    staged.sort_by_key(|s| std::cmp::Reverse(s.0));
    for (_line, e, d) in staged {
        edits.push(e);
        diags_touched.push(d);
    }

    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri.clone(), edits);
    let count = diags_touched.len();
    Some(CodeAction {
        title: format!(
            "Fix all Mighty problems ({} fix{})",
            count,
            if count == 1 { "" } else { "es" }
        ),
        kind: Some(CodeActionKind::new(SOURCE_FIX_ALL_MIGHTY)),
        diagnostics: Some(diags_touched),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    })
}

/// v0.35 T3 — Top-level handler taking the LSP `context.only` filter.
/// If the client requested `source.fixAll.mighty`, return the bulk
/// action; otherwise fall through to the per-diagnostic actions.
pub fn code_actions_with_filter(
    uri: &Url,
    doc: &DocAnalysis,
    cursor_range: Range,
    diagnostics: &[tower_lsp::lsp_types::Diagnostic],
    only: Option<&[CodeActionKind]>,
    cfg: CodeActionConfig,
) -> CodeActionResponse {
    code_actions_with_filter_caps(
        uri,
        doc,
        cursor_range,
        diagnostics,
        only,
        cfg,
        WorkspaceEditCaps::default(),
    )
}

/// v0.47 T5 — `code_actions_with_filter` honouring the `documentChanges`
/// capability. The server passes the negotiated caps in.
pub fn code_actions_with_filter_caps(
    uri: &Url,
    doc: &DocAnalysis,
    cursor_range: Range,
    diagnostics: &[tower_lsp::lsp_types::Diagnostic],
    only: Option<&[CodeActionKind]>,
    cfg: CodeActionConfig,
    caps: WorkspaceEditCaps,
) -> CodeActionResponse {
    // If the client explicitly asked for `source.fixAll.mighty`, emit
    // only that action (per LSP semantics: `only` is a strict filter).
    if let Some(kinds) = only {
        if kinds.iter().any(|k| k.as_str() == SOURCE_FIX_ALL_MIGHTY) {
            return match fix_all_mighty_action(uri, doc, cfg) {
                Some(ca) => {
                    let resp: CodeActionResponse = vec![CodeActionOrCommand::CodeAction(ca)];
                    if caps.document_changes {
                        upgrade_response_to_document_changes(resp, doc.version)
                    } else {
                        resp
                    }
                }
                None => vec![],
            };
        }
    }
    code_actions_with_caps(uri, doc, cursor_range, diagnostics, cfg, caps)
}

fn actions_for_unknown_macro(
    uri: &Url,
    doc: &DocAnalysis,
    diag: &tower_lsp::lsp_types::Diagnostic,
    out: &mut Vec<CodeActionOrCommand>,
) {
    let bad = extract_name_from_range(doc, diag.range);
    if bad.is_empty() {
        return;
    }
    // Macros are top-level items lowered into the HIR; their names are
    // surfaced via Item::Macro nodes. Collect them by walking the
    // package's items so the suggestion list reflects the real set
    // of macros visible in this file.
    let mut macro_names: Vec<String> = Vec::new();
    for iid in &doc.package.top_level {
        if let mty_hir::Item::Macro(m) = &doc.package.items[*iid] {
            macro_names.push(m.name.clone());
        }
    }
    let sugg = nearest(&bad, &macro_names);
    for s in sugg {
        out.push(quickfix_replace(
            uri,
            diag.clone(),
            diag.range,
            s.clone(),
            format!("Replace macro `{}` with `{}`", bad, s),
        ));
    }
}

fn diag_contains_cursor(d: &Diagnostic, doc: &DocAnalysis, cursor: Range) -> bool {
    let r = crate::conv::span_to_range(
        &doc.line_index,
        &doc.source,
        d.primary.start as u32,
        d.primary.end as u32,
    );
    !(r.end.line < cursor.start.line || r.start.line > cursor.end.line)
}

fn actions_for_unresolved_value(
    uri: &Url,
    doc: &DocAnalysis,
    diag: &tower_lsp::lsp_types::Diagnostic,
    out: &mut Vec<CodeActionOrCommand>,
) {
    let bad = extract_name_from_range(doc, diag.range);
    if bad.is_empty() {
        return;
    }
    let candidates: Vec<String> = doc
        .typed
        .def_map
        .by_name
        .iter()
        .filter(|(_, r)| matches!(r, DefRef::Fn(_) | DefRef::Variant(_, _) | DefRef::Module(_)))
        .map(|(k, _)| k.clone())
        .collect();
    let suggestions = nearest(&bad, &candidates);
    for sugg in suggestions {
        out.push(quickfix_replace(
            uri,
            diag.clone(),
            diag.range,
            sugg.clone(),
            format!("Replace `{}` with `{}`", bad, sugg),
        ));
    }
}

fn actions_for_unresolved_type(
    uri: &Url,
    doc: &DocAnalysis,
    diag: &tower_lsp::lsp_types::Diagnostic,
    out: &mut Vec<CodeActionOrCommand>,
) {
    let bad = extract_name_from_range(doc, diag.range);
    if bad.is_empty() {
        return;
    }
    let candidates: Vec<String> = doc
        .typed
        .def_map
        .by_name
        .iter()
        .filter(|(_, r)| matches!(r, DefRef::Adt(_) | DefRef::Param(_)))
        .map(|(k, _)| k.clone())
        .collect();
    let suggestions = nearest(&bad, &candidates);
    for sugg in suggestions {
        out.push(quickfix_replace(
            uri,
            diag.clone(),
            diag.range,
            sugg.clone(),
            format!("Replace type `{}` with `{}`", bad, sugg),
        ));
    }
}

fn actions_for_use_after_move(
    uri: &Url,
    doc: &DocAnalysis,
    diag: &tower_lsp::lsp_types::Diagnostic,
    out: &mut Vec<CodeActionOrCommand>,
) {
    // Pragmatic: suggest inserting `.clone()` after the moved expression.
    // The user gets to verify in the preview before applying — for non-
    // Copy types without a Clone impl this won't compile and they'll
    // see the new diagnostic.
    let target = extract_name_from_range(doc, diag.range);
    if target.is_empty() {
        return;
    }
    let end = diag.range.end;
    let edit = TextEdit {
        range: Range { start: end, end },
        new_text: ".clone()".to_string(),
    };
    out.push(quickfix_with_edit(
        uri,
        diag.clone(),
        vec![edit],
        format!("Add `.clone()` after `{}`", target),
    ));
}

fn actions_for_effect_undeclared(
    uri: &Url,
    doc: &DocAnalysis,
    diag: &tower_lsp::lsp_types::Diagnostic,
    out: &mut Vec<CodeActionOrCommand>,
) {
    // We don't have a precise "missing effect name" parser, but the
    // diagnostic's message typically embeds it. Extract a quoted name.
    let effect = extract_quoted(&diag.message).unwrap_or_else(|| "io".to_string());
    // Locate the enclosing fn and decide where to insert.
    let Some(insert_pos) = find_effect_insert_pos(doc, diag.range) else {
        return;
    };
    let edit = TextEdit {
        range: Range {
            start: insert_pos,
            end: insert_pos,
        },
        new_text: format!(" effect {{ {} }}", effect),
    };
    out.push(quickfix_with_edit(
        uri,
        diag.clone(),
        vec![edit],
        format!("Add `effect {{ {} }}` to fn signature", effect),
    ));
}

fn find_effect_insert_pos(doc: &DocAnalysis, diag_range: Range) -> Option<Position> {
    use mty_syntax::{SyntaxKind, SyntaxNode};
    let offset = doc.line_index.position_to_offset(
        &doc.source,
        diag_range.start.line,
        diag_range.start.character,
    );
    let root = SyntaxNode::new_root(doc.parsed.green.clone());
    // Find the smallest enclosing FN_DECL that contains the offset.
    let pos = rowan::TextSize::from(offset);
    let mut best: Option<SyntaxNode> = None;
    for n in root.descendants() {
        if n.kind() == SyntaxKind::FN_DECL && n.text_range().contains(pos) {
            match &best {
                None => best = Some(n.clone()),
                Some(prev) if n.text_range().len() < prev.text_range().len() => {
                    best = Some(n.clone())
                }
                _ => {}
            }
        }
    }
    let fn_decl = best?;
    // Insert before the body block.
    let block = fn_decl.children().find(|c| c.kind() == SyntaxKind::BLOCK)?;
    let insert_byte: u32 = block.text_range().start().into();
    let (line, character) = doc.line_index.offset_to_position(&doc.source, insert_byte);
    Some(Position { line, character })
}

fn extract_quoted(msg: &str) -> Option<String> {
    // Pull the first backtick-delimited or single-quote-delimited token.
    let mut chars = msg.chars();
    while let Some(c) = chars.next() {
        if c == '`' || c == '\'' {
            let close = c;
            let s: String = chars.by_ref().take_while(|c| *c != close).collect();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

fn extract_name_from_range(doc: &DocAnalysis, range: Range) -> String {
    let start =
        doc.line_index
            .position_to_offset(&doc.source, range.start.line, range.start.character)
            as usize;
    let end =
        doc.line_index
            .position_to_offset(&doc.source, range.end.line, range.end.character) as usize;
    if start > end || end > doc.source.len() {
        return String::new();
    }
    doc.source[start..end].to_string()
}

fn quickfix_replace(
    uri: &Url,
    diag: tower_lsp::lsp_types::Diagnostic,
    range: Range,
    new_text: String,
    title: String,
) -> CodeActionOrCommand {
    let edit = TextEdit { range, new_text };
    quickfix_with_edit(uri, diag, vec![edit], title)
}

fn quickfix_with_edit(
    uri: &Url,
    diag: tower_lsp::lsp_types::Diagnostic,
    edits: Vec<TextEdit>,
    title: String,
) -> CodeActionOrCommand {
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri.clone(), edits);
    CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    })
}

/// Return the up to 3 closest candidates to `target` by edit distance,
/// filtering to those within [`SUGGEST_MAX_DISTANCE`].
fn nearest(target: &str, candidates: &[String]) -> Vec<String> {
    let mut scored: Vec<(usize, &String)> = candidates
        .iter()
        .filter(|c| c.as_str() != target)
        .map(|c| (edit_distance(target, c), c))
        .filter(|(d, _)| *d <= SUGGEST_MAX_DISTANCE)
        .collect();
    scored.sort_by_key(|(d, _)| *d);
    scored.into_iter().take(3).map(|(_, c)| c.clone()).collect()
}

/// Levenshtein distance with the standard O(m*n) DP.
fn edit_distance(a: &str, b: &str) -> usize {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let m = ac.len();
    let n = bc.len();
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur: Vec<usize> = vec![0; n + 1];
    for i in 1..=m {
        cur[0] = i;
        for j in 1..=n {
            let cost = if ac[i - 1] == bc[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[n]
}

#[cfg(test)]
mod tests {
    use super::*;
    use mty_diagnostics::fix::{Fix, FixBuilder, FixKind, SpanInfo};
    use serde_json::json;
    use tower_lsp::lsp_types as lt;

    #[test]
    fn distance_basics() {
        assert_eq!(edit_distance("foo", "foo"), 0);
        assert_eq!(edit_distance("foo", "fob"), 1);
        assert_eq!(edit_distance("foo", "fo"), 1);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn nearest_picks_within_threshold() {
        let cands: Vec<String> = ["greet", "great", "creep", "wildly_different"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let n = nearest("grret", &cands);
        assert!(n.contains(&"greet".to_string()));
        assert!(!n.contains(&"wildly_different".to_string()));
    }

    #[test]
    fn extract_quoted_picks_first() {
        assert_eq!(extract_quoted("missing `io` effect"), Some("io".into()));
        assert_eq!(extract_quoted("no quotes here"), None);
    }

    // ---------- v0.34 T2 — config + envelope ----------

    #[test]
    fn config_default_thresholds() {
        let cfg = CodeActionConfig::default();
        assert!((cfg.visible_threshold - DEFAULT_VISIBLE_THRESHOLD).abs() < f32::EPSILON);
        assert!((cfg.preferred_threshold - DEFAULT_PREFERRED_THRESHOLD).abs() < f32::EPSILON);
    }

    #[test]
    fn config_from_init_options_reads_threshold() {
        let opts = json!({
            "mighty": {
                "codeAction": { "confidenceThreshold": 0.5 }
            }
        });
        let cfg = CodeActionConfig::from_initialization_options(&opts);
        assert!((cfg.visible_threshold - 0.5).abs() < f32::EPSILON);
        // Preferred threshold stays at the default (still ≥ visible).
        assert!(cfg.preferred_threshold >= cfg.visible_threshold);
    }

    #[test]
    fn config_from_init_options_ignores_out_of_range() {
        let opts = json!({"mighty": {"codeAction": {"confidenceThreshold": 2.0}}});
        let cfg = CodeActionConfig::from_initialization_options(&opts);
        assert!((cfg.visible_threshold - DEFAULT_VISIBLE_THRESHOLD).abs() < f32::EPSILON);
    }

    #[test]
    fn config_from_init_options_handles_missing_key() {
        let cfg = CodeActionConfig::from_initialization_options(&json!({}));
        assert!((cfg.visible_threshold - DEFAULT_VISIBLE_THRESHOLD).abs() < f32::EPSILON);
    }

    #[test]
    fn config_threshold_at_root_level_also_works() {
        // Some clients omit the "mighty" wrapper.
        let opts = json!({"codeAction": {"confidenceThreshold": 0.8}});
        let cfg = CodeActionConfig::from_initialization_options(&opts);
        assert!((cfg.visible_threshold - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn code_action_kind_for_untaint_is_quickfix() {
        let k = code_action_kind_for("untaint");
        assert_eq!(k, CodeActionKind::QUICKFIX);
    }

    #[test]
    fn code_action_kind_for_rename_is_refactor_rewrite() {
        let k = code_action_kind_for("rename_to_match_decl");
        assert_eq!(k, CodeActionKind::REFACTOR_REWRITE);
    }

    #[test]
    fn code_action_kind_for_add_match_arm_is_refactor() {
        assert_eq!(
            code_action_kind_for("add_match_arm"),
            CodeActionKind::REFACTOR_REWRITE
        );
    }

    #[test]
    fn supported_kinds_include_quickfix_refactor_and_fix_all() {
        let kinds = supported_code_action_kinds();
        assert!(kinds.contains(&CodeActionKind::QUICKFIX));
        assert!(kinds.contains(&CodeActionKind::REFACTOR_REWRITE));
        assert!(kinds.iter().any(|k| k.as_str() == SOURCE_FIX_ALL_MIGHTY));
    }

    fn synthetic_diag(code: &str, range: Range) -> tower_lsp::lsp_types::Diagnostic {
        tower_lsp::lsp_types::Diagnostic {
            range,
            code: Some(NumberOrString::String(code.into())),
            message: format!("synthetic {}", code),
            ..Default::default()
        }
    }

    fn synthetic_envelope_with_three_alts(line: u32) -> DiagnosticEnvelope {
        let alt_a = FixBuilder::new("alt A", "r", 0.92)
            .replace_line("u.mty", line, "  call(tainted)", "  call(safe_a)")
            .build();
        let alt_b = FixBuilder::new("alt B", "r", 0.80)
            .replace_line("u.mty", line, "  call(tainted)", "  call(safe_b)")
            .build();
        let alt_c = FixBuilder::new("alt C", "r", 0.60)
            .replace_line("u.mty", line, "  call(tainted)", "  call(safe_c)")
            .build();
        DiagnosticEnvelope {
            schema_version: mty_diagnostics::fix::SCHEMA_VERSION.to_string(),
            code: "MT4099".into(),
            severity: "error".into(),
            span: SpanInfo {
                file: "u.mty".into(),
                line,
                col: 1,
                len: 1,
                byte_start: 0,
                byte_end: 1,
            },
            title: "tainted".into(),
            prose: "p".into(),
            fix: Some(Fix {
                kind: FixKind::Untaint.as_str().to_string(),
                confidence: 0.92,
                alternatives: vec![alt_a, alt_b, alt_c],
            }),
            see_also: vec![],
            source: None,
        }
    }

    #[test]
    fn emit_envelope_actions_default_threshold_drops_low_confidence() {
        let env = synthetic_envelope_with_three_alts(1);
        let source = "  call(tainted)\n";
        let mut out: Vec<CodeActionOrCommand> = Vec::new();
        let uri = lt::Url::parse("test://u.mty").unwrap();
        let lsp_diag = synthetic_diag(
            "MT4099",
            Range {
                start: lt::Position {
                    line: 0,
                    character: 0,
                },
                end: lt::Position {
                    line: 0,
                    character: 1,
                },
            },
        );
        emit_envelope_actions(
            &uri,
            &env,
            &lsp_diag,
            source,
            CodeActionConfig::default(),
            &std::collections::HashSet::new(),
            &mut out,
        );
        // 0.92 and 0.80 survive; 0.60 is dropped.
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn emit_envelope_actions_lowered_threshold_includes_low_confidence() {
        let env = synthetic_envelope_with_three_alts(1);
        let source = "  call(tainted)\n";
        let mut out: Vec<CodeActionOrCommand> = Vec::new();
        let uri = lt::Url::parse("test://u.mty").unwrap();
        let diag = synthetic_diag(
            "MT4099",
            Range {
                start: lt::Position {
                    line: 0,
                    character: 0,
                },
                end: lt::Position {
                    line: 0,
                    character: 1,
                },
            },
        );
        let cfg = CodeActionConfig {
            visible_threshold: 0.5,
            preferred_threshold: 0.85,
        };
        emit_envelope_actions(
            &uri,
            &env,
            &diag,
            source,
            cfg,
            &std::collections::HashSet::new(),
            &mut out,
        );
        // All 3 alternatives visible now.
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn emit_envelope_actions_marks_high_confidence_as_preferred() {
        let env = synthetic_envelope_with_three_alts(1);
        let source = "  call(tainted)\n";
        let mut out: Vec<CodeActionOrCommand> = Vec::new();
        let uri = lt::Url::parse("test://u.mty").unwrap();
        let diag = synthetic_diag(
            "MT4099",
            Range {
                start: lt::Position {
                    line: 0,
                    character: 0,
                },
                end: lt::Position {
                    line: 0,
                    character: 1,
                },
            },
        );
        emit_envelope_actions(
            &uri,
            &env,
            &diag,
            source,
            CodeActionConfig::default(),
            &std::collections::HashSet::new(),
            &mut out,
        );
        // First action (confidence 0.92 ≥ 0.85) is preferred.
        // Second action (0.80 < 0.85) is not preferred.
        let prefs: Vec<bool> = out
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.is_preferred.unwrap_or(false),
                _ => false,
            })
            .collect();
        assert_eq!(prefs, vec![true, false]);
    }

    #[test]
    fn emit_envelope_actions_assigns_quickfix_kind_to_untaint() {
        let env = synthetic_envelope_with_three_alts(1);
        let source = "  call(tainted)\n";
        let mut out: Vec<CodeActionOrCommand> = Vec::new();
        let uri = lt::Url::parse("test://u.mty").unwrap();
        let diag = synthetic_diag(
            "MT4099",
            Range {
                start: lt::Position {
                    line: 0,
                    character: 0,
                },
                end: lt::Position {
                    line: 0,
                    character: 1,
                },
            },
        );
        emit_envelope_actions(
            &uri,
            &env,
            &diag,
            source,
            CodeActionConfig::default(),
            &std::collections::HashSet::new(),
            &mut out,
        );
        for a in &out {
            match a {
                CodeActionOrCommand::CodeAction(ca) => {
                    assert_eq!(ca.kind.as_ref(), Some(&CodeActionKind::QUICKFIX));
                }
                _ => panic!("expected CodeAction"),
            }
        }
    }

    #[test]
    fn emit_envelope_actions_skips_diff_that_does_not_match_source() {
        // Envelope says line 1 is "  call(tainted)" but our source has
        // "  call(other)". The diff should refuse to apply.
        let env = synthetic_envelope_with_three_alts(1);
        let source = "  call(other)\n";
        let mut out: Vec<CodeActionOrCommand> = Vec::new();
        let uri = lt::Url::parse("test://u.mty").unwrap();
        let diag = synthetic_diag(
            "MT4099",
            Range {
                start: lt::Position {
                    line: 0,
                    character: 0,
                },
                end: lt::Position {
                    line: 0,
                    character: 1,
                },
            },
        );
        emit_envelope_actions(
            &uri,
            &env,
            &diag,
            source,
            CodeActionConfig::default(),
            &std::collections::HashSet::new(),
            &mut out,
        );
        assert!(out.is_empty(), "should drop fixes that don't apply cleanly");
    }

    #[test]
    fn emit_envelope_actions_dedupes_against_hand_written_titles() {
        let env = synthetic_envelope_with_three_alts(1);
        let source = "  call(tainted)\n";
        let mut out: Vec<CodeActionOrCommand> = Vec::new();
        let uri = lt::Url::parse("test://u.mty").unwrap();
        let diag = synthetic_diag(
            "MT4099",
            Range {
                start: lt::Position {
                    line: 0,
                    character: 0,
                },
                end: lt::Position {
                    line: 0,
                    character: 1,
                },
            },
        );
        // Pre-populate the dedup set with the title the envelope would emit.
        let mut seen = std::collections::HashSet::new();
        seen.insert("Apply: alt A".to_string());
        emit_envelope_actions(
            &uri,
            &env,
            &diag,
            source,
            CodeActionConfig::default(),
            &seen,
            &mut out,
        );
        // Only alt B survives (alt A skipped, alt C below threshold).
        assert_eq!(out.len(), 1);
        if let CodeActionOrCommand::CodeAction(ca) = &out[0] {
            assert_eq!(ca.title, "Apply: alt B");
        }
    }

    #[test]
    fn envelope_action_carries_workspace_edit() {
        let uri = lt::Url::parse("test://u.mty").unwrap();
        let edits = vec![TextEdit {
            range: Range {
                start: lt::Position {
                    line: 0,
                    character: 0,
                },
                end: lt::Position {
                    line: 0,
                    character: 0,
                },
            },
            new_text: "x".into(),
        }];
        let diag = synthetic_diag(
            "MT0001",
            Range {
                start: lt::Position {
                    line: 0,
                    character: 0,
                },
                end: lt::Position {
                    line: 0,
                    character: 0,
                },
            },
        );
        let a = envelope_action(
            &uri,
            diag,
            edits,
            "t".into(),
            CodeActionKind::QUICKFIX,
            true,
        );
        if let CodeActionOrCommand::CodeAction(ca) = a {
            let we = ca.edit.expect("edit");
            let changes = we.changes.expect("changes");
            assert!(changes.contains_key(&uri));
            assert_eq!(ca.is_preferred, Some(true));
        } else {
            panic!("expected CodeAction");
        }
    }
}
