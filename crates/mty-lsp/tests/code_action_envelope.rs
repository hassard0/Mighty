//! v0.34 T2 — envelope-driven CodeAction integration tests.
//!
//! The pipeline under test:
//!
//! ```text
//!     mty-diagnostics::fix  (envelope + unified diff)
//!         → mty-lsp::diff_apply  (unified diff → LSP TextEdit[])
//!         → mty-lsp::code_actions::code_actions  (CodeAction[])
//! ```
//!
//! The hand-written scope-aware fixes still ship via `code_action_unresolved`;
//! these tests focus on the envelope path (MT4099 untaint as the
//! marquee), plus confidence-threshold filtering at the LSP boundary.

use mty_diagnostics::{
    codes::TAINTED_VALUE_TO_SINK, diagnostic::Label, Diagnostic as MtyDiagnostic, Severity,
};
use mty_lsp::code_actions::{code_actions, code_actions_with_config, CodeActionConfig};
use mty_lsp::docs::DocAnalysis;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, NumberOrString, Position, Range,
    TextDocumentContentChangeEvent, Url,
};

fn analyze_with_diag(source: &str, diag: MtyDiagnostic) -> DocAnalysis {
    let mut doc = DocAnalysis::analyze(source.to_string(), "test://m.mty".into(), 1);
    doc.diagnostics.push(diag);
    doc
}

fn uri() -> Url {
    Url::parse("test://m.mty").unwrap()
}

fn _ensure_change_event_can_be_constructed() {
    // Just so the import of TextDocumentContentChangeEvent stays useful
    // (silences dead-code lints if the test list shrinks).
    let _ = TextDocumentContentChangeEvent {
        range: None,
        range_length: None,
        text: String::new(),
    };
}

#[test]
fn mt4099_envelope_emits_three_untaint_alternatives() {
    // The classic MT4099 line: a tainted identifier flowing into a
    // sink-shaped call. T1's fix engine returns three untaint
    // alternatives (regex / sanitizer / allowlist) — the LSP should
    // surface all three as CodeActions.
    let src = "tainted_input\n";
    let diag = MtyDiagnostic {
        code: TAINTED_VALUE_TO_SINK,
        severity: Severity::Error,
        primary: Label {
            start: 0,
            end: src.trim_end().len(),
            message: "tainted value flows to sink".into(),
        },
        secondary: vec![],
        notes: vec![],
        helps: vec![],
    };
    let doc = analyze_with_diag(src, diag);

    // Construct a synthetic LSP diagnostic for the same span (this is
    // what the editor would round-trip back).
    let lsp_diag = tower_lsp::lsp_types::Diagnostic {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: src.trim_end().len() as u32,
            },
        },
        code: Some(NumberOrString::String("MT4099".into())),
        message: "tainted".into(),
        ..Default::default()
    };

    let cfg = CodeActionConfig {
        // Pull in all three alternatives by lowering the visible
        // threshold below T1's lowest alternative confidence (0.85).
        visible_threshold: 0.5,
        preferred_threshold: 0.85,
    };
    let actions = code_actions_with_config(
        &uri(),
        &doc,
        Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 1,
            },
        },
        &[lsp_diag],
        cfg,
    );
    let envelope_actions: Vec<&CodeAction> = actions
        .iter()
        .filter_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Apply: ") => Some(ca),
            _ => None,
        })
        .collect();

    assert_eq!(
        envelope_actions.len(),
        3,
        "MT4099 should surface 3 untaint strategies; got titles {:?}",
        envelope_actions
            .iter()
            .map(|a| &a.title)
            .collect::<Vec<_>>()
    );

    // The marquee strategies all carry the QUICKFIX kind.
    for a in &envelope_actions {
        assert_eq!(
            a.kind.as_ref(),
            Some(&CodeActionKind::QUICKFIX),
            "untaint strategies should be quickfixes"
        );
    }

    // The highest-confidence alternative (0.92, regex) is preferred.
    let preferred_count = envelope_actions
        .iter()
        .filter(|a| a.is_preferred.unwrap_or(false))
        .count();
    assert!(
        preferred_count >= 1,
        "at least one alt should be preferred (confidence ≥ 0.85)"
    );
}

#[test]
fn mt4099_envelope_default_threshold_keeps_high_confidence_alts() {
    // At the default visible_threshold of 0.7, T1's untaint trio
    // (0.92 / 0.90 / 0.85) all survive.
    let src = "tainted_input\n";
    let diag = MtyDiagnostic {
        code: TAINTED_VALUE_TO_SINK,
        severity: Severity::Error,
        primary: Label {
            start: 0,
            end: src.trim_end().len(),
            message: "tainted value flows to sink".into(),
        },
        secondary: vec![],
        notes: vec![],
        helps: vec![],
    };
    let doc = analyze_with_diag(src, diag);
    let lsp_diag = tower_lsp::lsp_types::Diagnostic {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: src.trim_end().len() as u32,
            },
        },
        code: Some(NumberOrString::String("MT4099".into())),
        message: "tainted".into(),
        ..Default::default()
    };
    let actions = code_actions(
        &uri(),
        &doc,
        Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 1,
            },
        },
        &[lsp_diag],
    );
    let n = actions
        .iter()
        .filter(
            |a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Apply: ")),
        )
        .count();
    assert_eq!(n, 3, "default threshold (0.7) admits all three T1 alts");
}

#[test]
fn diff_apply_drops_action_when_envelope_diff_misaligns() {
    // To prove the LSP doesn't blindly trust envelope diffs, we feed
    // `unified_diff_to_text_edits` directly with a diff whose OLD
    // line doesn't match the source — the function should refuse.
    // (This is the safety net that protects users when the engine
    // and the live document drift apart.)
    use mty_diagnostics::fix::unified_diff;
    use mty_lsp::diff_apply::unified_diff_to_text_edits;

    let source = "actual line\n";
    let diff = unified_diff("x.mty", 1, &["different line"], &["replacement"]);
    let edits = unified_diff_to_text_edits(&diff, source);
    assert!(
        edits.is_empty(),
        "diff_apply must refuse to emit edits when OLD doesn't match source"
    );
}

#[test]
fn cursor_only_path_still_offers_envelope_fixes() {
    // The client may call codeAction with an empty diagnostic list
    // (just a cursor position). The LSP should still surface fixes
    // for any document-level diagnostic at that range.
    let src = "tainted_input\n";
    let diag = MtyDiagnostic {
        code: TAINTED_VALUE_TO_SINK,
        severity: Severity::Error,
        primary: Label {
            start: 0,
            end: src.trim_end().len(),
            message: "tainted".into(),
        },
        secondary: vec![],
        notes: vec![],
        helps: vec![],
    };
    let doc = analyze_with_diag(src, diag);
    let actions = code_actions(
        &uri(),
        &doc,
        Range {
            start: Position {
                line: 0,
                character: 3,
            },
            end: Position {
                line: 0,
                character: 3,
            },
        },
        &[],
    );
    let n = actions
        .iter()
        .filter(
            |a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Apply: ")),
        )
        .count();
    assert!(
        n > 0,
        "envelope path should fire on cursor-only invocation when a doc diag overlaps"
    );
}

#[test]
fn no_actions_when_cursor_is_far_from_diagnostic() {
    let src = "line1\nline2\nline3\n";
    let diag = MtyDiagnostic {
        code: TAINTED_VALUE_TO_SINK,
        severity: Severity::Error,
        primary: Label {
            start: 0,
            end: 5,
            message: "tainted".into(),
        },
        secondary: vec![],
        notes: vec![],
        helps: vec![],
    };
    let doc = analyze_with_diag(src, diag);
    let actions = code_actions(
        &uri(),
        &doc,
        Range {
            start: Position {
                line: 2,
                character: 0,
            },
            end: Position {
                line: 2,
                character: 0,
            },
        },
        &[],
    );
    assert!(
        actions.is_empty(),
        "cursor on line 3 shouldn't trigger fixes for a diag on line 1"
    );
}

#[test]
fn workspace_edit_carries_text_edit_for_correct_uri() {
    let src = "tainted_input\n";
    let diag = MtyDiagnostic {
        code: TAINTED_VALUE_TO_SINK,
        severity: Severity::Error,
        primary: Label {
            start: 0,
            end: src.trim_end().len(),
            message: "tainted".into(),
        },
        secondary: vec![],
        notes: vec![],
        helps: vec![],
    };
    let doc = analyze_with_diag(src, diag);
    let lsp_diag = tower_lsp::lsp_types::Diagnostic {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: src.trim_end().len() as u32,
            },
        },
        code: Some(NumberOrString::String("MT4099".into())),
        message: "tainted".into(),
        ..Default::default()
    };
    let actions = code_actions(
        &uri(),
        &doc,
        Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 1,
            },
        },
        &[lsp_diag],
    );
    let first = actions
        .iter()
        .find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Apply: ") => Some(ca),
            _ => None,
        })
        .expect("at least one envelope action");
    let we = first.edit.as_ref().expect("edit");
    let changes = we.changes.as_ref().expect("changes");
    assert!(
        changes.contains_key(&uri()),
        "WorkspaceEdit must key the edit by the document URI"
    );
    let edits = changes.get(&uri()).expect("edits");
    assert!(
        !edits.is_empty(),
        "the edit must carry at least one TextEdit"
    );
}

#[test]
fn config_threshold_above_all_alternatives_yields_nothing() {
    let src = "tainted_input\n";
    let diag = MtyDiagnostic {
        code: TAINTED_VALUE_TO_SINK,
        severity: Severity::Error,
        primary: Label {
            start: 0,
            end: src.trim_end().len(),
            message: "tainted".into(),
        },
        secondary: vec![],
        notes: vec![],
        helps: vec![],
    };
    let doc = analyze_with_diag(src, diag);
    let lsp_diag = tower_lsp::lsp_types::Diagnostic {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: src.trim_end().len() as u32,
            },
        },
        code: Some(NumberOrString::String("MT4099".into())),
        message: "tainted".into(),
        ..Default::default()
    };
    let cfg = CodeActionConfig {
        visible_threshold: 0.95, // above 0.92, the highest T1 alt
        preferred_threshold: 0.99,
    };
    let actions = code_actions_with_config(
        &uri(),
        &doc,
        Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 1,
            },
        },
        &[lsp_diag],
        cfg,
    );
    let envelope_actions = actions
        .iter()
        .filter(
            |a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Apply: ")),
        )
        .count();
    assert_eq!(envelope_actions, 0);
}
