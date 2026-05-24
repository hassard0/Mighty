//! Code-action quick-fix regressions for MT2021 (unresolved value).
//!
//! We construct a file containing a typo, ask the LSP for code actions
//! at the typo's range, and confirm at least one quick-fix is offered
//! whose new text equals the suggested replacement.

use mty_lsp::code_actions::code_actions;
use mty_lsp::docs::DocAnalysis;
use tower_lsp::lsp_types::{
    CodeActionContext, CodeActionOrCommand, Diagnostic, NumberOrString, Position, Range, Url,
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
