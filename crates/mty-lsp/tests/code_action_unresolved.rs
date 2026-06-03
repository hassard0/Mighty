//! Code-action quick-fix regressions for MT2021 (unresolved value).
//!
//! We construct a file containing a typo, ask the LSP for code actions
//! at the typo's range, and confirm at least one quick-fix is offered
//! whose new text equals the suggested replacement.

use mty_lsp::code_actions::{
    code_actions, code_actions_with_caps, CodeActionConfig, WorkspaceEditCaps,
};
use mty_lsp::docs::DocAnalysis;
use tower_lsp::lsp_types::{
    CodeActionContext, CodeActionOrCommand, Diagnostic, DocumentChanges, NumberOrString, Position,
    Range, Url,
};

fn analyze(src: &str) -> DocAnalysis {
    DocAnalysis::analyze(src.to_string(), "test://main.mty".to_string(), 1)
}

fn uri() -> Url {
    Url::parse("test://main.mty").unwrap()
}

fn _ctx(diags: Vec<Diagnostic>) -> CodeActionContext {
    CodeActionContext {
        diagnostics: diags,
        only: None,
        trigger_kind: None,
    }
}

#[test]
fn unresolved_value_emits_quickfix_with_suggestion() {
    // The file has a real fn `greet` and a typo'd call `gret`.
    let src = "fn greet() -> Unit { }\nfn main() { gret() }\n";
    let doc = analyze(src);
    // Find the typo's byte range.
    let bad = "gret";
    let off = src.find(bad).unwrap() as u32;
    let li = mty_lsp::line_index::LineIndex::new(src);
    let (line, character) = li.offset_to_position(src, off);
    let range = Range {
        start: Position { line, character },
        end: Position {
            line,
            character: character + bad.len() as u32,
        },
    };
    // Construct the synthetic MT2021 diagnostic the client would pass.
    let diag = Diagnostic {
        range,
        code: Some(NumberOrString::String("MT2021".into())),
        message: format!("unresolved value `{}`", bad),
        ..Default::default()
    };
    let actions = code_actions(&uri(), &doc, range, &[diag]);
    assert!(
        !actions.is_empty(),
        "expected at least one code action, got none"
    );
    // The top suggestion should replace `gret` with `greet`.
    let titles: Vec<String> = actions
        .iter()
        .map(|a| match a {
            CodeActionOrCommand::CodeAction(a) => a.title.clone(),
            _ => String::new(),
        })
        .collect();
    assert!(
        titles.iter().any(|t| t.contains("greet")),
        "expected a `greet` suggestion in titles {:?}",
        titles
    );
}

#[test]
fn no_diagnostics_no_actions_from_random_position() {
    let src = "fn main() { }\n";
    let doc = analyze(src);
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
        &[],
    );
    assert!(actions.is_empty());
}

// ---------- v0.47 T5 — documentChanges migration ----------

fn unresolved_actions(
    doc: &DocAnalysis,
    src: &str,
    caps: WorkspaceEditCaps,
) -> Vec<CodeActionOrCommand> {
    let bad = "gret";
    let off = src.find(bad).unwrap() as u32;
    let li = mty_lsp::line_index::LineIndex::new(src);
    let (line, character) = li.offset_to_position(src, off);
    let range = Range {
        start: Position { line, character },
        end: Position {
            line,
            character: character + bad.len() as u32,
        },
    };
    let diag = Diagnostic {
        range,
        code: Some(NumberOrString::String("MT2021".into())),
        message: format!("unresolved value `{}`", bad),
        ..Default::default()
    };
    code_actions_with_caps(
        &uri(),
        doc,
        range,
        &[diag],
        CodeActionConfig::default(),
        caps,
    )
}

#[test]
fn code_action_with_document_changes_emits_versioned_text_document_edit() {
    let src = "fn greet() -> Unit { }\nfn main() { gret() }\n";
    let doc = analyze(src);
    let actions = unresolved_actions(
        &doc,
        src,
        WorkspaceEditCaps {
            document_changes: true,
        },
    );
    assert!(!actions.is_empty(), "expected at least one action");
    // Every emitted CodeAction's WorkspaceEdit should use
    // `documentChanges` (no `changes`) and stamp the buffer version.
    for a in &actions {
        let CodeActionOrCommand::CodeAction(ca) = a else {
            continue;
        };
        let edit = ca.edit.as_ref().expect("edit");
        assert!(
            edit.changes.is_none(),
            "documentChanges path must not also populate `changes`"
        );
        let dc = edit.document_changes.as_ref().expect("documentChanges");
        let edits = match dc {
            DocumentChanges::Edits(e) => e,
            DocumentChanges::Operations(_) => panic!("expected Edits"),
        };
        assert_eq!(edits.len(), 1);
        let tde = &edits[0];
        assert_eq!(tde.text_document.uri, uri());
        assert_eq!(tde.text_document.version, Some(1));
    }
}

#[test]
fn code_action_without_document_changes_falls_back_to_legacy_changes() {
    // v0.46 T5 back-compat: client without documentChanges support
    // still sees the legacy `changes` map.
    let src = "fn greet() -> Unit { }\nfn main() { gret() }\n";
    let doc = analyze(src);
    let actions = unresolved_actions(
        &doc,
        src,
        WorkspaceEditCaps {
            document_changes: false,
        },
    );
    assert!(!actions.is_empty(), "expected at least one action");
    for a in &actions {
        let CodeActionOrCommand::CodeAction(ca) = a else {
            continue;
        };
        let edit = ca.edit.as_ref().expect("edit");
        assert!(
            edit.document_changes.is_none(),
            "downgraded shape must not emit documentChanges"
        );
        assert!(edit.changes.is_some(), "legacy `changes` must be populated");
    }
}
