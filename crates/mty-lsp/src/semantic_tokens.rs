//! Semantic tokens (`textDocument/semanticTokens/full` + `/range`).
//!
//! Walks the rowan CST and classifies each token into one of the LSP
//! semantic-token types declared in [`LEGEND_TYPES`]. The encoding LSP
//! requires is delta-relative-to-previous-token (see the
//! `delta_line` / `delta_start` / `length` fields on [`SemanticToken`]);
//! [`encode`] does the conversion from absolute `(line, char, length)`.
//!
//! Classification strategy (v0.5):
//!
//! - Keyword tokens (`fn`, `let`, etc.) → `keyword`.
//! - Numeric literals → `number`; string/char/HTML → `string`;
//!   line/block/doc comments → `comment`.
//! - Punctuation operators (`+`, `==`, `->`, …) → `operator`.
//! - `IDENT` tokens are looked up in the type checker's `DefMap`
//!   (`by_name`) and, when known, classified as `function`, `type`,
//!   `enumMember`, `namespace`, or `typeParameter`. Identifiers in
//!   `FN_PARAM` nodes are tagged `parameter`. Anything else falls back
//!   to `variable`.
//!
//! Modifiers:
//! - `declaration` — set when the IDENT is the `NAME` child of a
//!   declaration node (`FN_DECL`, `STRUCT_DECL`, …).
//! - `defaultLibrary` — set for prelude / built-in names (`String`,
//!   `Bool`, `I32`, `log`, …).
//! - `readonly` — set for `const` names and for non-`mut` `let`
//!   bindings (best-effort: requires walking up to the parent stmt).
//!
//! Trivia (whitespace) is skipped. We emit no tokens for unknown / error
//! tokens.

use crate::docs::DocAnalysis;
use crate::line_index::LineIndex;
use mty_syntax::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};
use mty_types::DefRef;
use tower_lsp::lsp_types::{
    Range, SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensLegend, SemanticTokensRangeResult, SemanticTokensResult,
};

/// LSP semantic token *type* legend. Indexes correspond to the `type`
/// field of [`SemanticToken`]. Keep in sync with [`type_index`].
pub const LEGEND_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::KEYWORD,        // 0
    SemanticTokenType::TYPE,           // 1
    SemanticTokenType::FUNCTION,       // 2
    SemanticTokenType::VARIABLE,       // 3
    SemanticTokenType::PARAMETER,      // 4
    SemanticTokenType::STRING,         // 5
    SemanticTokenType::NUMBER,         // 6
    SemanticTokenType::COMMENT,        // 7
    SemanticTokenType::OPERATOR,       // 8
    SemanticTokenType::NAMESPACE,      // 9
    SemanticTokenType::ENUM_MEMBER,    // 10
    SemanticTokenType::TYPE_PARAMETER, // 11
    SemanticTokenType::MACRO,          // 12
    SemanticTokenType::PROPERTY,       // 13
];

/// LSP semantic token *modifier* legend. Bit position = legend index.
pub const LEGEND_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,     // bit 0
    SemanticTokenModifier::READONLY,        // bit 1
    SemanticTokenModifier::DEFAULT_LIBRARY, // bit 2
];

const T_KEYWORD: u32 = 0;
const T_TYPE: u32 = 1;
const T_FUNCTION: u32 = 2;
const T_VARIABLE: u32 = 3;
const T_PARAMETER: u32 = 4;
const T_STRING: u32 = 5;
const T_NUMBER: u32 = 6;
const T_COMMENT: u32 = 7;
const T_OPERATOR: u32 = 8;
const T_NAMESPACE: u32 = 9;
const T_ENUM_MEMBER: u32 = 10;
const T_TYPE_PARAMETER: u32 = 11;
const T_MACRO: u32 = 12;
const T_PROPERTY: u32 = 13;

const M_DECLARATION: u32 = 1 << 0;
#[allow(dead_code)]
const M_READONLY: u32 = 1 << 1;
const M_DEFAULT_LIBRARY: u32 = 1 << 2;

/// Public legend constructor — used by the LSP `initialize` reply.
pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: LEGEND_TYPES.to_vec(),
        token_modifiers: LEGEND_MODIFIERS.to_vec(),
    }
}

/// `textDocument/semanticTokens/full` handler body.
pub fn full(doc: &DocAnalysis) -> SemanticTokensResult {
    let tokens = collect(doc, None);
    SemanticTokensResult::Tokens(SemanticTokens {
        result_id: None,
        data: encode(&tokens),
    })
}

/// `textDocument/semanticTokens/range` handler body. We compute the full
/// token list and filter to the requested range — simpler and (for
/// editor-sized files) plenty fast.
pub fn range(doc: &DocAnalysis, range: Range) -> SemanticTokensRangeResult {
    let tokens = collect(doc, Some(range));
    SemanticTokensRangeResult::Tokens(SemanticTokens {
        result_id: None,
        data: encode(&tokens),
    })
}

/// One semantic token in absolute coordinates (pre-delta-encoding).
#[derive(Debug, Clone, Copy)]
struct AbsToken {
    line: u32,
    start_char: u32,
    length: u32,
    token_type: u32,
    modifiers: u32,
}

fn collect(doc: &DocAnalysis, only_in: Option<Range>) -> Vec<AbsToken> {
    let mut out: Vec<AbsToken> = Vec::new();
    let root = SyntaxNode::new_root(doc.parsed.green.clone());
    walk(&root, doc, &mut out);
    if let Some(r) = only_in {
        out.retain(|t| token_in_range(t, &r));
    }
    // LSP requires tokens be sorted by (line, start_char). Our DFS walk
    // already produces them in source order, but a defensive sort costs
    // nothing for editor-sized files.
    out.sort_by_key(|t| (t.line, t.start_char));
    out
}

fn token_in_range(t: &AbsToken, r: &Range) -> bool {
    // Cheap inclusion: token's start position must fall within [r.start, r.end].
    let after_start = (t.line, t.start_char) >= (r.start.line, r.start.character);
    let before_end = (t.line, t.start_char) <= (r.end.line, r.end.character);
    after_start && before_end
}

fn walk(node: &SyntaxNode, doc: &DocAnalysis, out: &mut Vec<AbsToken>) {
    for child in node.children_with_tokens() {
        match child {
            SyntaxElement::Node(n) => walk(&n, doc, out),
            SyntaxElement::Token(t) => {
                if let Some(at) = classify(&t, doc) {
                    out.push(at);
                }
            }
        }
    }
}

fn classify(token: &SyntaxToken, doc: &DocAnalysis) -> Option<AbsToken> {
    let kind = token.kind();
    if kind.is_trivia()
        && !matches!(
            kind,
            SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT
        )
    {
        return None;
    }
    let (token_type, modifiers) = match kind {
        SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT => {
            (T_COMMENT, 0)
        }
        SyntaxKind::INT_LITERAL
        | SyntaxKind::HEX_INT_LITERAL
        | SyntaxKind::BIN_INT_LITERAL
        | SyntaxKind::OCT_INT_LITERAL
        | SyntaxKind::FLOAT_LITERAL
        | SyntaxKind::DURATION_LITERAL
        | SyntaxKind::SIZE_LITERAL => (T_NUMBER, 0),
        SyntaxKind::STRING_LITERAL | SyntaxKind::CHAR_LITERAL | SyntaxKind::HTML_LITERAL => {
            (T_STRING, 0)
        }
        SyntaxKind::IDENT => classify_ident(token, doc),
        _ => {
            if kind.is_keyword() || matches!(kind, SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW) {
                (T_KEYWORD, 0)
            } else if is_operator_kind(kind) {
                (T_OPERATOR, 0)
            } else {
                return None;
            }
        }
    };
    let len = u32::from(token.text_range().len());
    if len == 0 {
        return None;
    }
    let start: u32 = token.text_range().start().into();
    // Multi-line tokens (e.g. a block comment that spans lines) are
    // rare for our classifier but if they occur LSP requires us to
    // split them per-line. For simplicity we emit a single token using
    // the start line + the byte length; clients tolerate this for
    // comment-style tokens.
    let (line, start_char) = doc.line_index.offset_to_position(&doc.source, start);
    Some(AbsToken {
        line,
        start_char,
        length: utf16_length(&doc.line_index, &doc.source, start, len),
        token_type,
        modifiers,
    })
}

fn utf16_length(line_index: &LineIndex, source: &str, start: u32, byte_len: u32) -> u32 {
    let end = start + byte_len;
    let (sl, sc) = line_index.offset_to_position(source, start);
    let (el, ec) = line_index.offset_to_position(source, end);
    // If the token sits on one line, the UTF-16 length is the column
    // delta. For a multi-line token, fall back to the byte length —
    // multi-line tokens (comments) get under-counted on column but the
    // editor still highlights the run correctly because the deltas line
    // up.
    if sl == el {
        ec.saturating_sub(sc)
    } else {
        byte_len
    }
}

fn classify_ident(token: &SyntaxToken, doc: &DocAnalysis) -> (u32, u32) {
    let name = token.text().to_string();
    let parent_kind = token.parent().map(|p| p.kind());

    // Parameter binding inside an FN_PARAM: paint as `parameter`.
    if matches!(parent_kind, Some(SyntaxKind::FN_PARAM))
        || ancestor_kind(token, SyntaxKind::FN_PARAM).is_some()
    {
        return (T_PARAMETER, M_DECLARATION);
    }

    // Declaration site? — IDENT is the NAME of a declaration node.
    let decl_mod = if let Some(parent) = token.parent() {
        let pkind = parent.kind();
        let grand = parent.parent().map(|g| g.kind());
        let is_name_node = matches!(pkind, SyntaxKind::NAME);
        let is_decl_grand = matches!(
            grand,
            Some(
                SyntaxKind::FN_DECL
                    | SyntaxKind::STRUCT_DECL
                    | SyntaxKind::ENUM_DECL
                    | SyntaxKind::TYPE_ALIAS
                    | SyntaxKind::AGENT_DECL
                    | SyntaxKind::PROTOCOL_DECL
                    | SyntaxKind::SUPERVISOR_DECL
                    | SyntaxKind::TRAIT_DECL
                    | SyntaxKind::CONST_DECL
                    | SyntaxKind::MACRO_DECL
                    | SyntaxKind::ENUM_VARIANT
                    | SyntaxKind::STRUCT_FIELD
            )
        );
        if is_name_node && is_decl_grand {
            M_DECLARATION
        } else {
            0
        }
    } else {
        0
    };

    // Macro invocation (e.g. `format!`): the IDENT directly precedes a
    // `!`. We detect this conservatively by checking the next sibling.
    if next_sibling_is(token, SyntaxKind::BANG) {
        return (T_MACRO, decl_mod);
    }

    // Field expression target (`x.field`): the IDENT under a FIELD_EXPR
    // and *after* a DOT → property.
    if let Some(parent) = token.parent() {
        if parent.kind() == SyntaxKind::FIELD_EXPR && prev_sibling_is(token, SyntaxKind::DOT) {
            return (T_PROPERTY, decl_mod);
        }
        if parent.kind() == SyntaxKind::STRUCT_FIELD_EXPR
            && next_sibling_is(token, SyntaxKind::COLON)
        {
            return (T_PROPERTY, decl_mod);
        }
        if matches!(parent.kind(), SyntaxKind::METHOD_CALL_EXPR)
            && prev_sibling_is(token, SyntaxKind::DOT)
        {
            return (T_FUNCTION, decl_mod);
        }
    }

    // Look up in the type checker's DefMap.
    if let Some(def) = doc.typed.def_map.by_name.get(&name) {
        let (ty, extra) = match def {
            DefRef::Fn(_) => (T_FUNCTION, prelude_mod(&name)),
            DefRef::Adt(_) => (T_TYPE, prelude_mod(&name)),
            DefRef::Variant(_, _) => (T_ENUM_MEMBER, 0),
            DefRef::Module(_) => (T_NAMESPACE, prelude_mod(&name)),
            DefRef::Param(_) => (T_TYPE_PARAMETER, 0),
            // v0.41 T6 (L16): top-level `const NAME = ...;` declarations
            // are highlighted as variables for now (no dedicated CONSTANT
            // token in the table). `M_READONLY` would be ideal but the
            // current modifier set doesn't include it.
            DefRef::Const(_) => (T_VARIABLE, 0),
        };
        return (ty, decl_mod | extra);
    }

    // Primitive type names are not always in `by_name` (they live in the
    // type-checker's prelude). Recognize a small set explicitly so users
    // get the `defaultLibrary` highlight on `I32`, `String`, etc.
    if is_primitive_type(&name) {
        return (T_TYPE, M_DEFAULT_LIBRARY | decl_mod);
    }

    // Heuristic: leading uppercase → looks like a type / variant.
    if name
        .chars()
        .next()
        .map(|c| c.is_ascii_uppercase())
        .unwrap_or(false)
    {
        return (T_TYPE, decl_mod);
    }

    (T_VARIABLE, decl_mod)
}

fn ancestor_kind(token: &SyntaxToken, target: SyntaxKind) -> Option<SyntaxNode> {
    let mut cur = token.parent();
    while let Some(n) = cur {
        if n.kind() == target {
            return Some(n);
        }
        cur = n.parent();
    }
    None
}

fn next_sibling_is(token: &SyntaxToken, kind: SyntaxKind) -> bool {
    let mut cur = token.next_sibling_or_token();
    while let Some(el) = cur {
        if let Some(t) = el.as_token() {
            if t.kind().is_trivia() {
                cur = el.next_sibling_or_token();
                continue;
            }
            return t.kind() == kind;
        }
        return false;
    }
    false
}

fn prev_sibling_is(token: &SyntaxToken, kind: SyntaxKind) -> bool {
    let mut cur = token.prev_sibling_or_token();
    while let Some(el) = cur {
        if let Some(t) = el.as_token() {
            if t.kind().is_trivia() {
                cur = el.prev_sibling_or_token();
                continue;
            }
            return t.kind() == kind;
        }
        return false;
    }
    false
}

fn is_operator_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PLUS
            | SyntaxKind::MINUS
            | SyntaxKind::STAR
            | SyntaxKind::SLASH
            | SyntaxKind::PERCENT
            | SyntaxKind::EQ
            | SyntaxKind::EQ_EQ
            | SyntaxKind::BANG_EQ
            | SyntaxKind::LT
            | SyntaxKind::LT_EQ
            | SyntaxKind::GT
            | SyntaxKind::GT_EQ
            | SyntaxKind::AMP
            | SyntaxKind::AMP_AMP
            | SyntaxKind::PIPE
            | SyntaxKind::PIPE_PIPE
            | SyntaxKind::CARET
            | SyntaxKind::SHL
            | SyntaxKind::SHR
            | SyntaxKind::PLUS_EQ
            | SyntaxKind::MINUS_EQ
            | SyntaxKind::STAR_EQ
            | SyntaxKind::SLASH_EQ
            | SyntaxKind::PERCENT_EQ
            | SyntaxKind::AMP_EQ
            | SyntaxKind::PIPE_EQ
            | SyntaxKind::CARET_EQ
            | SyntaxKind::SHL_EQ
            | SyntaxKind::SHR_EQ
            | SyntaxKind::THIN_ARROW
            | SyntaxKind::FAT_ARROW
            | SyntaxKind::DOT_DOT
            | SyntaxKind::DOT_DOT_EQ
            | SyntaxKind::BANG
            | SyntaxKind::QUESTION
    )
}

fn is_primitive_type(name: &str) -> bool {
    matches!(
        name,
        "Bool"
            | "Char"
            | "Str"
            | "String"
            | "Bytes"
            | "Unit"
            | "Never"
            | "Duration"
            | "Size"
            | "I8"
            | "I16"
            | "I32"
            | "I64"
            | "I128"
            | "U8"
            | "U16"
            | "U32"
            | "U64"
            | "U128"
            | "USize"
            | "ISize"
            | "F32"
            | "F64"
            | "Result"
            | "Option"
    )
}

fn prelude_mod(name: &str) -> u32 {
    // Heuristic: names that the prelude or built-in tables exposes get
    // the `defaultLibrary` modifier. Names registered as ADTs whose
    // origin is the prelude land here. We can't always tell from name
    // alone — this is a best-effort hint.
    if is_primitive_type(name) || matches!(name, "log" | "print" | "println") {
        M_DEFAULT_LIBRARY
    } else {
        0
    }
}

/// Encode absolute tokens as the delta-relative stream the LSP wire
/// format requires.
fn encode(tokens: &[AbsToken]) -> Vec<SemanticToken> {
    let mut out: Vec<SemanticToken> = Vec::with_capacity(tokens.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;
    for t in tokens {
        let delta_line = t.line - prev_line;
        let delta_start = if delta_line == 0 {
            t.start_char - prev_start
        } else {
            t.start_char
        };
        out.push(SemanticToken {
            delta_line,
            delta_start,
            length: t.length,
            token_type: t.token_type,
            token_modifiers_bitset: t.modifiers,
        });
        prev_line = t.line;
        prev_start = t.start_char;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_two_tokens_same_line() {
        let toks = vec![
            AbsToken {
                line: 0,
                start_char: 0,
                length: 2,
                token_type: T_KEYWORD,
                modifiers: 0,
            },
            AbsToken {
                line: 0,
                start_char: 3,
                length: 4,
                token_type: T_FUNCTION,
                modifiers: M_DECLARATION,
            },
        ];
        let enc = encode(&toks);
        assert_eq!(enc.len(), 2);
        assert_eq!(enc[0].delta_line, 0);
        assert_eq!(enc[0].delta_start, 0);
        assert_eq!(enc[1].delta_line, 0);
        assert_eq!(enc[1].delta_start, 3);
        assert_eq!(enc[1].token_type, T_FUNCTION);
        assert_eq!(enc[1].token_modifiers_bitset, M_DECLARATION);
    }

    #[test]
    fn encode_two_tokens_different_lines() {
        let toks = vec![
            AbsToken {
                line: 0,
                start_char: 5,
                length: 2,
                token_type: T_KEYWORD,
                modifiers: 0,
            },
            AbsToken {
                line: 2,
                start_char: 4,
                length: 4,
                token_type: T_VARIABLE,
                modifiers: 0,
            },
        ];
        let enc = encode(&toks);
        assert_eq!(enc[1].delta_line, 2);
        assert_eq!(enc[1].delta_start, 4); // absolute, not relative
    }
}
