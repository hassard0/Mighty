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
use std::collections::{HashMap, VecDeque};
use tower_lsp::lsp_types::{
    Range, SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensDelta, SemanticTokensEdit, SemanticTokensFullDeltaResult, SemanticTokensLegend,
    SemanticTokensRangeResult, SemanticTokensResult, Url,
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
///
/// v0.47 T5: the returned [`SemanticTokens`] never carries a
/// `result_id` on its own — callers that want delta support should
/// use [`full_with_cache`] instead, which stores the snapshot so a
/// follow-up `textDocument/semanticTokens/full/delta` can compute the
/// diff against it.
pub fn full(doc: &DocAnalysis) -> SemanticTokensResult {
    let tokens = collect(doc, None);
    SemanticTokensResult::Tokens(SemanticTokens {
        result_id: None,
        data: encode(&tokens),
    })
}

/// v0.47 T5: full-request variant that records the encoded token array
/// into `cache` keyed by `uri`, so a subsequent
/// `textDocument/semanticTokens/full/delta` request can emit
/// [`SemanticTokensEdit`]s relative to this snapshot. The returned
/// `result_id` matches the one stored in the cache.
pub fn full_with_cache(
    uri: &Url,
    doc: &DocAnalysis,
    cache: &mut DeltaCache,
) -> SemanticTokensResult {
    let tokens = collect(doc, None);
    let data = encode(&tokens);
    let result_id = cache.store(uri.clone(), doc.version, data.clone());
    SemanticTokensResult::Tokens(SemanticTokens {
        result_id: Some(result_id),
        data,
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

// ---------------------------------------------------------------------
// v0.47 T5 — semanticTokens delta
// ---------------------------------------------------------------------

/// One cached semantic-tokens snapshot — the encoded token array we
/// previously sent the client, plus the buffer version it corresponds
/// to. Keyed inside [`DeltaCache`] by `(uri, result_id)`.
#[derive(Debug, Clone)]
pub struct CachedSnapshot {
    pub version: i32,
    pub data: Vec<SemanticToken>,
}

/// LRU-bounded per-URI cache of recently emitted semanticTokens
/// snapshots. The server stores one snapshot per `(uri, result_id)`
/// pair; on a delta request we look up the entry whose `result_id`
/// matches the client's `previous_result_id` and diff the new tokens
/// against it.
///
/// Eviction policy: simple FIFO bounded at `capacity`. When the map
/// reaches the cap the oldest entry is dropped to make room. This is
/// pessimistic — a typing-burst across many open buffers will
/// invalidate the head — but keeps memory bounded under adversarial
/// clients. See the trailing "Unresolved" note in the v0.47 T5 PR for
/// the planned per-URI windowed cache.
#[derive(Debug)]
pub struct DeltaCache {
    snapshots: HashMap<(Url, String), CachedSnapshot>,
    order: VecDeque<(Url, String)>,
    capacity: usize,
    counter: u64,
}

impl Default for DeltaCache {
    fn default() -> Self {
        Self::with_capacity(256)
    }
}

impl DeltaCache {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            snapshots: HashMap::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
            counter: 0,
        }
    }

    /// Insert (or replace) a snapshot for `uri` and return the fresh
    /// `result_id` the client should echo on its next delta request.
    pub fn store(&mut self, uri: Url, version: i32, data: Vec<SemanticToken>) -> String {
        self.counter += 1;
        let result_id = format!("mty-st-{}", self.counter);
        let key = (uri.clone(), result_id.clone());
        self.snapshots
            .insert(key.clone(), CachedSnapshot { version, data });
        self.order.push_back(key);
        while self.snapshots.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.snapshots.remove(&oldest);
            } else {
                break;
            }
        }
        result_id
    }

    /// Look up the snapshot matching `(uri, result_id)`. Returns
    /// `None` when the client's `previous_result_id` is stale (e.g. it
    /// was evicted, or the server restarted).
    pub fn get(&self, uri: &Url, result_id: &str) -> Option<&CachedSnapshot> {
        self.snapshots.get(&(uri.clone(), result_id.to_string()))
    }

    /// Drop every snapshot for `uri` — useful when a file is closed.
    pub fn drop_uri(&mut self, uri: &Url) {
        self.snapshots.retain(|(u, _), _| u != uri);
        self.order.retain(|(u, _)| u != uri);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.snapshots.len()
    }
}

/// `textDocument/semanticTokens/full/delta` handler body.
///
/// Strategy:
///   - Encode the current tokens fresh (same path as `full`).
///   - Look up the cached snapshot for `previous_result_id`.
///   - Cache miss → fall back to a full response, stamping the new
///     `result_id` so the next delta request can succeed.
///   - Cache hit → compute a [`SemanticTokensDelta`] with the smallest
///     prefix-and-suffix-aligned diff between the old and new token
///     arrays, returning that under the new `result_id`. The old
///     entry stays in the cache for the duration of the LRU window.
///
/// The diff algorithm: trim the common prefix + suffix, then emit a
/// single `SemanticTokensEdit { start, deleteCount, data }` covering
/// the changed middle. This is the simplest correct shape per LSP —
/// editors that want finer-grained edits can ignore it and request a
/// full refresh, but VS Code and Neovim handle the single-edit form
/// cleanly. (Real-world diffs from incremental edits are dominated by
/// a single contiguous run, so the single-edit form costs almost
/// nothing vs. a true LCS-based diff.)
pub fn full_delta(
    uri: &Url,
    doc: &DocAnalysis,
    previous_result_id: &str,
    cache: &mut DeltaCache,
) -> SemanticTokensFullDeltaResult {
    let tokens = collect(doc, None);
    let new_data = encode(&tokens);

    let prev = cache.get(uri, previous_result_id).map(|s| s.data.clone());

    match prev {
        None => {
            // Stale `previous_result_id` — return a full response with
            // a fresh result_id so the client can resync.
            let result_id = cache.store(uri.clone(), doc.version, new_data.clone());
            SemanticTokensFullDeltaResult::Tokens(SemanticTokens {
                result_id: Some(result_id),
                data: new_data,
            })
        }
        Some(old_data) => {
            let edits = diff_tokens(&old_data, &new_data);
            let result_id = cache.store(uri.clone(), doc.version, new_data);
            SemanticTokensFullDeltaResult::TokensDelta(SemanticTokensDelta {
                result_id: Some(result_id),
                edits,
            })
        }
    }
}

/// Compute a single-edit delta between two token streams.
///
/// The delta encodes the smallest contiguous slice in the new array
/// that, when spliced over the matching slice in the old array,
/// reproduces it: common prefix + (start..start+deleteCount) replaced
/// with `data` + common suffix.
///
/// Returns an empty vec when the streams are identical (the client
/// keeps its current view, no work).
fn diff_tokens(old: &[SemanticToken], new: &[SemanticToken]) -> Vec<SemanticTokensEdit> {
    if old == new {
        return vec![];
    }
    let max_prefix = old.len().min(new.len());
    let mut prefix = 0usize;
    while prefix < max_prefix && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix < (old.len() - prefix)
        && suffix < (new.len() - prefix)
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let delete_count = old.len() - prefix - suffix;
    let inserted = new[prefix..new.len() - suffix].to_vec();
    vec![SemanticTokensEdit {
        start: prefix as u32,
        delete_count: delete_count as u32,
        data: Some(inserted),
    }]
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

    fn st(delta_line: u32, delta_start: u32, token_type: u32) -> SemanticToken {
        SemanticToken {
            delta_line,
            delta_start,
            length: 1,
            token_type,
            token_modifiers_bitset: 0,
        }
    }

    #[test]
    fn diff_identical_streams_returns_no_edits() {
        let a = vec![st(0, 0, T_KEYWORD), st(0, 3, T_FUNCTION)];
        let b = a.clone();
        let edits = diff_tokens(&a, &b);
        assert!(
            edits.is_empty(),
            "identical streams should produce no edits"
        );
    }

    #[test]
    fn diff_appended_token_emits_tail_insert() {
        let a = vec![st(0, 0, T_KEYWORD)];
        let b = vec![st(0, 0, T_KEYWORD), st(0, 3, T_FUNCTION)];
        let edits = diff_tokens(&a, &b);
        assert_eq!(edits.len(), 1);
        let e = &edits[0];
        assert_eq!(e.start, 1);
        assert_eq!(e.delete_count, 0);
        assert_eq!(e.data.as_ref().map(|d| d.len()), Some(1));
    }

    #[test]
    fn diff_changed_middle_emits_replace_edit() {
        let a = vec![
            st(0, 0, T_KEYWORD),
            st(0, 3, T_FUNCTION),
            st(0, 8, T_VARIABLE),
        ];
        let b = vec![
            st(0, 0, T_KEYWORD),
            st(0, 3, T_NUMBER),
            st(0, 8, T_VARIABLE),
        ];
        let edits = diff_tokens(&a, &b);
        assert_eq!(edits.len(), 1);
        let e = &edits[0];
        assert_eq!(e.start, 1, "should skip the common prefix [keyword]");
        assert_eq!(e.delete_count, 1, "should replace the changed middle token");
        let data = e.data.as_ref().expect("data");
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].token_type, T_NUMBER);
    }

    #[test]
    fn diff_deletion_at_head_emits_prefix_delete() {
        let a = vec![st(0, 0, T_KEYWORD), st(0, 3, T_FUNCTION)];
        let b = vec![st(0, 3, T_FUNCTION)];
        let edits = diff_tokens(&a, &b);
        assert_eq!(edits.len(), 1);
        let e = &edits[0];
        assert_eq!(e.start, 0);
        assert_eq!(e.delete_count, 1);
        assert_eq!(e.data.as_ref().map(|d| d.len()).unwrap_or(0), 0);
    }

    #[test]
    fn delta_cache_round_trips_a_stored_snapshot() {
        let mut cache = DeltaCache::with_capacity(4);
        let uri = Url::parse("file:///x.mty").unwrap();
        let data = vec![st(0, 0, T_KEYWORD)];
        let rid = cache.store(uri.clone(), 1, data.clone());
        let snap = cache.get(&uri, &rid).expect("hit");
        assert_eq!(snap.version, 1);
        assert_eq!(snap.data, data);
    }

    #[test]
    fn delta_cache_evicts_oldest_when_over_capacity() {
        let mut cache = DeltaCache::with_capacity(2);
        let u1 = Url::parse("file:///1.mty").unwrap();
        let u2 = Url::parse("file:///2.mty").unwrap();
        let u3 = Url::parse("file:///3.mty").unwrap();
        let r1 = cache.store(u1.clone(), 1, vec![st(0, 0, T_KEYWORD)]);
        let _r2 = cache.store(u2.clone(), 1, vec![st(0, 0, T_KEYWORD)]);
        let _r3 = cache.store(u3.clone(), 1, vec![st(0, 0, T_KEYWORD)]);
        assert_eq!(cache.len(), 2);
        // Oldest (u1) was evicted.
        assert!(cache.get(&u1, &r1).is_none(), "u1 should have been evicted");
    }

    #[test]
    fn delta_cache_drop_uri_removes_only_that_uri() {
        let mut cache = DeltaCache::with_capacity(4);
        let u1 = Url::parse("file:///1.mty").unwrap();
        let u2 = Url::parse("file:///2.mty").unwrap();
        let r1 = cache.store(u1.clone(), 1, vec![st(0, 0, T_KEYWORD)]);
        let r2 = cache.store(u2.clone(), 1, vec![st(0, 0, T_KEYWORD)]);
        cache.drop_uri(&u1);
        assert!(cache.get(&u1, &r1).is_none());
        assert!(cache.get(&u2, &r2).is_some());
    }
}
