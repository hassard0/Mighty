//! v0.35 T5 — "Strategy B" stdlib hover catalog.
//!
//! ## Why
//!
//! v0.33 T6 + v0.34 T3 shipped a 203-entry hand-curated table
//! ([`crate::examples::STDLIB_EXAMPLES`]) hard-coded in Rust. That
//! design was always meant as a temporary bridge — the v0.33 T6 plan
//! called it "Strategy A". v0.35 T5 ships Strategy B: a real
//! source-of-truth that lives next to the stdlib it describes, parsed
//! at compile time into the same [`crate::examples::StdlibExample`]
//! shape.
//!
//! ## Pipeline
//!
//! 1. Per-module `_doc.mty`-style stub files at
//!    `crates/mty-stdlib/docs/<module>.docstub`. Each file is the
//!    source-of-truth for that module's hover catalog.
//! 2. This module's [`parse_docstub`] reads each file's
//!    mini-grammar (one entry per `##sym ... ##end` block) and
//!    returns an owned vector of catalog entries.
//! 3. [`build_extracted_catalog`] glues the per-module results into a
//!    flat catalog in declaration order, matching the curated table's
//!    shape exactly so the LSP can swap one for the other without
//!    touching the hover renderer.
//!
//! ## Docstub format
//!
//! Line-oriented; one entry per `##sym ... ##end` block; blank lines
//! and lines starting with `# ` (a single `#` followed by space) are
//! comments. Order of directives WITHIN a block is free except
//! `##example` must be last.
//!
//! ```text
//! ##module llm
//!
//! ##sym Member.ask
//! ##sig fn Member.ask(&self, prompt: Str) -> Result<MemberReply, LlmError>
//! ##cap net.https (for the provider endpoint)
//! ##desc Sends prompt to the LLM provider and returns the reply.
//! ##see Member.anthropic, Member.openai, std.swarm, swarm
//! ##example
//! let m = Member.anthropic("claude-opus-4-7");
//! let r = m.ask("Capital of France?")?;
//! log(r.text);
//! ##end
//! ```
//!
//! Missing `##cap` is allowed (capability-free symbols); the field
//! is parsed as the empty string and matches the curated convention.
//!
//! ## Drift gate
//!
//! [`mty doc check`](../../../../mty-cli/src/cmd/doc.rs) compares the
//! extracted catalog against the curated [`STDLIB_EXAMPLES`] and exits
//! non-zero on any divergence (missing/extra symbol, or signature /
//! capability / example body / see-also drift). CI runs this gate so
//! a stdlib change that touches behaviour without updating the docstub
//! fails the merge.

use crate::examples::StdlibExample;

/// Owned counterpart to [`StdlibExample`]. The curated table is
/// `&'static str`-only; the extractor produces owned strings (its
/// source is the docstub text, which lives as `&'static str` via
/// `include_str!`, but parsed sub-slices need owned copies once we
/// trim and concatenate `\n`-joined example bodies).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedExample {
    pub symbol: String,
    pub signature: String,
    pub description: String,
    pub capability: String,
    pub example: String,
    pub see_also: String,
    /// Stdlib module bucket the entry was extracted from (e.g. `llm`,
    /// `swarm`, `rag`). Useful for `mty doc check` reports and for the
    /// LSP to surface module breadcrumbs in hover.
    pub module: String,
}

impl ExtractedExample {
    /// Borrow as the same shape the LSP hover renderer expects.
    pub fn as_stdlib_example(&self) -> StdlibExampleRef<'_> {
        StdlibExampleRef {
            symbol: &self.symbol,
            signature: &self.signature,
            description: &self.description,
            capability: &self.capability,
            example: &self.example,
            see_also: &self.see_also,
        }
    }
}

/// Borrowed view that matches the field shape of [`StdlibExample`]
/// but holds `&str` (not `&'static str`). Lets the LSP hover renderer
/// consume extracted entries without rebuilding the markdown helpers.
#[derive(Debug, Clone, Copy)]
pub struct StdlibExampleRef<'a> {
    pub symbol: &'a str,
    pub signature: &'a str,
    pub description: &'a str,
    pub capability: &'a str,
    pub example: &'a str,
    pub see_also: &'a str,
}

/// One parsed docstub file. The walker uses this for diagnostics
/// (file_name + line numbers in error reports) and to preserve
/// declaration order across modules.
#[derive(Debug)]
pub struct DocstubFile<'a> {
    pub module: &'a str,
    pub entries: Vec<ExtractedExample>,
}

/// Parse error from [`parse_docstub`]. Carries the line number (1-based)
/// + a short diagnostic for surface-quality CI output.
#[derive(Debug, PartialEq, Eq)]
pub struct DocstubError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for DocstubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for DocstubError {}

/// Parse a single `.docstub` file body into an owned entry list.
///
/// `default_module` is used when the file omits a `##module` header
/// (no v0.35 file does, but the walker is tolerant). When `##module`
/// IS present it overrides `default_module`.
pub fn parse_docstub<'a>(
    src: &'a str,
    default_module: &'a str,
) -> Result<DocstubFile<'a>, DocstubError> {
    let mut entries: Vec<ExtractedExample> = Vec::new();
    let mut module: Option<String> = None;

    let mut iter = src.lines().enumerate().peekable();

    while let Some(&(_lineno, line)) = iter.peek() {
        // Skip blanks and full-line comments (`# ...` — note the
        // required space; `##` is a directive).
        if line.trim().is_empty()
            || (line.starts_with("# ") && !line.starts_with("##"))
            || line.trim() == "#"
        {
            iter.next();
            continue;
        }
        if let Some(rest) = line.strip_prefix("##module ") {
            module = Some(rest.trim().to_string());
            iter.next();
            continue;
        }
        if line.starts_with("##generated_from ") {
            iter.next();
            continue;
        }
        if line.starts_with("##sym ") {
            let entry = parse_entry(&mut iter)?;
            entries.push(entry);
            continue;
        }
        return Err(DocstubError {
            line: iter.peek().map(|(n, _)| n + 1).unwrap_or(0),
            message: format!("unexpected line outside an entry block: {line:?}"),
        });
    }

    // We return the module slice from `src` when the `##module`
    // directive is present, else fall back to `default_module`. We
    // borrow rather than allocate so callers can use the same lifetime.
    let module_slice: &str = if module.is_some() {
        find_module_directive(src).unwrap_or(default_module)
    } else {
        default_module
    };
    // Stamp the extracted module onto every entry so consumers don't
    // need to thread it.
    let module_owned = module_slice.to_string();
    for e in entries.iter_mut() {
        e.module.clone_from(&module_owned);
    }
    Ok(DocstubFile {
        module: module_slice,
        entries,
    })
}

/// Borrow the `##module` directive's value as a slice of `src`.
fn find_module_directive(src: &str) -> Option<&str> {
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("##module ") {
            return Some(rest.trim());
        }
    }
    None
}

fn parse_entry<'a, I>(iter: &mut std::iter::Peekable<I>) -> Result<ExtractedExample, DocstubError>
where
    I: Iterator<Item = (usize, &'a str)>,
{
    let (start_lineno, first) = iter.next().expect("caller peeked ##sym");
    let symbol = first
        .strip_prefix("##sym ")
        .ok_or_else(|| DocstubError {
            line: start_lineno + 1,
            message: format!("expected `##sym <name>`, got {first:?}"),
        })?
        .trim()
        .to_string();

    let mut signature = String::new();
    let mut description = String::new();
    let mut capability = String::new();
    let mut see_also = String::new();
    let mut example = String::new();
    let mut saw_end = false;

    while let Some(&(lineno, line)) = iter.peek() {
        if let Some(rest) = line.strip_prefix("##sig ") {
            signature = rest.trim().to_string();
            iter.next();
        } else if let Some(rest) = line.strip_prefix("##desc ") {
            description = rest.trim().to_string();
            iter.next();
        } else if let Some(rest) = line.strip_prefix("##cap ") {
            capability = rest.trim().to_string();
            iter.next();
        } else if let Some(rest) = line.strip_prefix("##see ") {
            see_also = rest.trim().to_string();
            iter.next();
        } else if line == "##example" {
            iter.next();
            // Collect every subsequent line until `##end`.
            loop {
                match iter.next() {
                    Some((_, "##end")) => {
                        saw_end = true;
                        break;
                    }
                    Some((_, body)) => {
                        if !example.is_empty() {
                            example.push('\n');
                        }
                        example.push_str(body);
                    }
                    None => {
                        return Err(DocstubError {
                            line: lineno + 1,
                            message: "unterminated `##example` block (missing `##end`)".to_string(),
                        })
                    }
                }
            }
            break;
        } else if line == "##end" {
            // No example block — `##end` closes the entry directly.
            iter.next();
            saw_end = true;
            break;
        } else if line.trim().is_empty() {
            // Allow blank lines inside an entry header.
            iter.next();
        } else {
            return Err(DocstubError {
                line: lineno + 1,
                message: format!("unexpected directive inside entry {symbol:?}: {line:?}"),
            });
        }
    }

    if !saw_end {
        return Err(DocstubError {
            line: start_lineno + 1,
            message: format!("unterminated entry block for {symbol:?} (missing `##end`)"),
        });
    }

    if signature.is_empty() {
        return Err(DocstubError {
            line: start_lineno + 1,
            message: format!("entry {symbol:?} missing required `##sig` directive"),
        });
    }

    // Curated convention: example bodies end with a single trailing
    // newline. Re-append one so the extracted vs. curated comparison
    // is byte-for-byte clean.
    if !example.is_empty() && !example.ends_with('\n') {
        example.push('\n');
    }

    Ok(ExtractedExample {
        symbol,
        signature,
        description,
        capability,
        example,
        see_also,
        module: String::new(), // stamped by caller from the file header
    })
}

// ---------------------------------------------------------------------
// Compile-time embedding of the on-disk docstubs.
// ---------------------------------------------------------------------

/// The embedded docstub bodies. Each `(module, src)` tuple is the
/// `include_str!` of `crates/mty-stdlib/docs/<module>.docstub`. Adding
/// a new stdlib surface = adding a new line here + a new file.
///
/// Order matters: it is the iteration order [`build_extracted_catalog`]
/// uses to flatten the catalog. Keeping it stable preserves
/// declaration-order parity with the curated `STDLIB_EXAMPLES`.
pub const EMBEDDED_DOCSTUBS: &[(&str, &str)] = &[
    ("llm", include_str!("../../mty-stdlib/docs/llm.docstub")),
    ("swarm", include_str!("../../mty-stdlib/docs/swarm.docstub")),
    ("mcp", include_str!("../../mty-stdlib/docs/mcp.docstub")),
    (
        "memory",
        include_str!("../../mty-stdlib/docs/memory.docstub"),
    ),
    ("eval", include_str!("../../mty-stdlib/docs/eval.docstub")),
    (
        "observe",
        include_str!("../../mty-stdlib/docs/observe.docstub"),
    ),
    ("http", include_str!("../../mty-stdlib/docs/http.docstub")),
    ("fs", include_str!("../../mty-stdlib/docs/fs.docstub")),
    ("time", include_str!("../../mty-stdlib/docs/time.docstub")),
    (
        "builtin",
        include_str!("../../mty-stdlib/docs/builtin.docstub"),
    ),
    ("json", include_str!("../../mty-stdlib/docs/json.docstub")),
    (
        "string",
        include_str!("../../mty-stdlib/docs/string.docstub"),
    ),
    ("vec", include_str!("../../mty-stdlib/docs/vec.docstub")),
    ("env", include_str!("../../mty-stdlib/docs/env.docstub")),
    ("rag", include_str!("../../mty-stdlib/docs/rag.docstub")),
    (
        "computer",
        include_str!("../../mty-stdlib/docs/computer.docstub"),
    ),
    ("web", include_str!("../../mty-stdlib/docs/web.docstub")),
    ("taint", include_str!("../../mty-stdlib/docs/taint.docstub")),
];

/// Parse every embedded docstub and return the flat catalog.
///
/// Panics on a parse error — the docstubs are bundled at compile time
/// and are not user input, so a malformed file is a build-time bug
/// that the CI gate must catch. The companion test
/// `extracted_catalog_parses_clean` asserts this for every PR.
pub fn build_extracted_catalog() -> Vec<ExtractedExample> {
    let mut out: Vec<ExtractedExample> = Vec::new();
    for (module, src) in EMBEDDED_DOCSTUBS {
        match parse_docstub(src, module) {
            Ok(file) => {
                out.extend(file.entries);
            }
            Err(e) => {
                panic!("docstub {module:?} failed to parse: {e}");
            }
        }
    }
    out
}

/// Look up an extracted entry by qualified symbol or bare ident.
pub fn lookup_extracted<'a>(
    catalog: &'a [ExtractedExample],
    name: &str,
) -> Option<&'a ExtractedExample> {
    if name.contains('.') {
        return catalog.iter().find(|e| e.symbol == name);
    }
    if let Some(exact) = catalog.iter().find(|e| e.symbol == name) {
        return Some(exact);
    }
    catalog
        .iter()
        .find(|e| e.symbol.rsplit('.').next() == Some(name))
}

// ---------------------------------------------------------------------
// Drift detection: compare extracted to the curated gold-set.
// ---------------------------------------------------------------------

/// One difference between the curated and extracted catalogs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftKind {
    /// In curated table but not produced by the extractor.
    MissingFromExtracted,
    /// Produced by the extractor but not in the curated table.
    ExtraInExtracted,
    /// Both sides have the symbol but a field disagrees.
    FieldMismatch { field: &'static str },
}

/// One drift entry surfaced to `mty doc check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    pub symbol: String,
    pub kind: DriftKind,
    pub curated: String,
    pub extracted: String,
}

/// Compare the extracted catalog (left) to the curated table (right)
/// and return every disagreement in stable order.
pub fn diff_catalogs(extracted: &[ExtractedExample], curated: &[StdlibExample]) -> Vec<Drift> {
    let mut out: Vec<Drift> = Vec::new();
    let curated_by_symbol: std::collections::HashMap<&str, &StdlibExample> =
        curated.iter().map(|e| (e.symbol, e)).collect();
    let extracted_by_symbol: std::collections::HashMap<&str, &ExtractedExample> =
        extracted.iter().map(|e| (e.symbol.as_str(), e)).collect();

    // Pass 1: missing-from-extracted (curated has it, extracted does not).
    for c in curated {
        if !extracted_by_symbol.contains_key(c.symbol) {
            out.push(Drift {
                symbol: c.symbol.to_string(),
                kind: DriftKind::MissingFromExtracted,
                curated: c.symbol.to_string(),
                extracted: "(missing)".to_string(),
            });
        }
    }
    // Pass 2: extra-in-extracted (extracted has it, curated does not).
    for e in extracted {
        if !curated_by_symbol.contains_key(e.symbol.as_str()) {
            out.push(Drift {
                symbol: e.symbol.clone(),
                kind: DriftKind::ExtraInExtracted,
                curated: "(missing)".to_string(),
                extracted: e.symbol.clone(),
            });
        }
    }
    // Pass 3: field mismatches for symbols present on both sides.
    for c in curated {
        let Some(e) = extracted_by_symbol.get(c.symbol) else {
            continue;
        };
        if c.signature != e.signature {
            out.push(Drift {
                symbol: c.symbol.to_string(),
                kind: DriftKind::FieldMismatch { field: "signature" },
                curated: c.signature.to_string(),
                extracted: e.signature.clone(),
            });
        }
        if c.description != e.description {
            out.push(Drift {
                symbol: c.symbol.to_string(),
                kind: DriftKind::FieldMismatch {
                    field: "description",
                },
                curated: c.description.to_string(),
                extracted: e.description.clone(),
            });
        }
        if c.capability != e.capability {
            out.push(Drift {
                symbol: c.symbol.to_string(),
                kind: DriftKind::FieldMismatch {
                    field: "capability",
                },
                curated: c.capability.to_string(),
                extracted: e.capability.clone(),
            });
        }
        if c.example != e.example {
            out.push(Drift {
                symbol: c.symbol.to_string(),
                kind: DriftKind::FieldMismatch { field: "example" },
                curated: c.example.to_string(),
                extracted: e.example.clone(),
            });
        }
        if c.see_also != e.see_also {
            out.push(Drift {
                symbol: c.symbol.to_string(),
                kind: DriftKind::FieldMismatch { field: "see_also" },
                curated: c.see_also.to_string(),
                extracted: e.see_also.clone(),
            });
        }
    }
    out
}

/// Render a drift report in the same stable Markdown shape used by
/// `mty doc check` and the CI gate. Empty `drift` yields the empty
/// string (caller prints a success banner separately).
pub fn render_drift_report(drift: &[Drift]) -> String {
    if drift.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    let mut missing = 0usize;
    let mut extra = 0usize;
    let mut mismatch = 0usize;
    for d in drift {
        match d.kind {
            DriftKind::MissingFromExtracted => missing += 1,
            DriftKind::ExtraInExtracted => extra += 1,
            DriftKind::FieldMismatch { .. } => mismatch += 1,
        }
    }
    s.push_str(&format!(
        "drift detected: {missing} missing, {extra} extra, {mismatch} field-mismatch\n\n"
    ));
    for d in drift {
        match d.kind {
            DriftKind::MissingFromExtracted => {
                s.push_str(&format!(
                    "  missing: {} (curated table has it, no docstub)\n",
                    d.symbol
                ));
            }
            DriftKind::ExtraInExtracted => {
                s.push_str(&format!(
                    "  extra:   {} (docstub has it, curated table does not)\n",
                    d.symbol
                ));
            }
            DriftKind::FieldMismatch { field } => {
                s.push_str(&format!("  drift:   {} :: {} differs\n", d.symbol, field));
                s.push_str(&format!("    curated:   {:?}\n", d.curated));
                s.push_str(&format!("    extracted: {:?}\n", d.extracted));
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::examples::STDLIB_EXAMPLES;

    #[test]
    fn parses_minimal_entry() {
        let src = "\
##module llm

##sym Member.ask
##sig fn Member.ask() -> Reply
##desc one liner
##see Member.openai
##example
let r = m.ask(\"hi\")?;
##end
";
        let file = parse_docstub(src, "llm").unwrap();
        assert_eq!(file.module, "llm");
        assert_eq!(file.entries.len(), 1);
        let e = &file.entries[0];
        assert_eq!(e.symbol, "Member.ask");
        assert_eq!(e.signature, "fn Member.ask() -> Reply");
        assert_eq!(e.description, "one liner");
        assert_eq!(e.capability, "");
        assert_eq!(e.see_also, "Member.openai");
        assert_eq!(e.example, "let r = m.ask(\"hi\")?;\n");
        assert_eq!(e.module, "llm");
    }

    #[test]
    fn parses_entry_with_capability() {
        let src = "\
##sym Member.anthropic
##sig fn Member.anthropic(model: Str) -> Member
##cap net.https (api.anthropic.com)
##desc Constructs an Anthropic panel member.
##see Member.openai
##example
let m = Member.anthropic(\"claude\");
##end
";
        let file = parse_docstub(src, "llm").unwrap();
        let e = &file.entries[0];
        assert_eq!(e.capability, "net.https (api.anthropic.com)");
    }

    #[test]
    fn parses_multiline_example_block() {
        let src = "\
##sym swarm
##sig fn swarm() -> Consensus
##desc multi-member
##see Member.anthropic
##example
let panel = [a, b];
let c = swarm(p, panel, b, S).await?;
log(c.body);
##end
";
        let file = parse_docstub(src, "swarm").unwrap();
        let e = &file.entries[0];
        assert_eq!(
            e.example,
            "let panel = [a, b];\nlet c = swarm(p, panel, b, S).await?;\nlog(c.body);\n"
        );
    }

    #[test]
    fn errors_on_missing_end() {
        let src = "\
##sym foo
##sig fn foo()
##desc x
##see y
##example
body
";
        let err = parse_docstub(src, "x").unwrap_err();
        assert!(err.message.contains("unterminated"));
    }

    #[test]
    fn errors_on_missing_signature() {
        let src = "\
##sym foo
##desc x
##see y
##end
";
        let err = parse_docstub(src, "x").unwrap_err();
        assert!(err.message.contains("missing required `##sig`"));
    }

    #[test]
    fn extracted_catalog_parses_clean() {
        // The embedded docstubs must always parse — they ship as part of
        // the binary.
        let catalog = build_extracted_catalog();
        assert!(
            catalog.len() >= 200,
            "expected >= 200 extracted entries, got {}",
            catalog.len()
        );
    }

    #[test]
    fn extracted_catalog_has_zero_drift_against_curated() {
        // The v0.35 T5 gate: extracted ≡ curated, byte-for-byte across
        // every field. If this fails, either the docstubs drifted or
        // the curated table did.
        let extracted = build_extracted_catalog();
        let drift = diff_catalogs(&extracted, STDLIB_EXAMPLES);
        assert!(
            drift.is_empty(),
            "drift detected (Strategy B regression!):\n{}",
            render_drift_report(&drift)
        );
    }

    #[test]
    fn extracted_catalog_count_matches_curated() {
        let extracted = build_extracted_catalog();
        assert_eq!(
            extracted.len(),
            STDLIB_EXAMPLES.len(),
            "extracted count != curated count (drift)"
        );
    }

    #[test]
    fn lookup_extracted_matches_curated_lookup() {
        let catalog = build_extracted_catalog();
        for c in STDLIB_EXAMPLES {
            let e = lookup_extracted(&catalog, c.symbol)
                .unwrap_or_else(|| panic!("extracted has no entry for {}", c.symbol));
            assert_eq!(e.signature, c.signature);
        }
    }

    #[test]
    fn drift_kind_field_mismatch_is_emitted() {
        let cur = [StdlibExample {
            symbol: "X.y",
            signature: "fn X.y() -> Unit",
            description: "old",
            capability: "",
            example: "",
            see_also: "",
        }];
        let ext = vec![ExtractedExample {
            symbol: "X.y".to_string(),
            signature: "fn X.y() -> Unit".to_string(),
            description: "new".to_string(),
            capability: "".to_string(),
            example: "".to_string(),
            see_also: "".to_string(),
            module: "test".to_string(),
        }];
        let d = diff_catalogs(&ext, &cur);
        assert_eq!(d.len(), 1);
        assert!(matches!(
            d[0].kind,
            DriftKind::FieldMismatch {
                field: "description"
            }
        ));
    }

    #[test]
    fn drift_kind_missing_is_emitted() {
        let cur = [StdlibExample {
            symbol: "X.y",
            signature: "fn",
            description: "",
            capability: "",
            example: "",
            see_also: "",
        }];
        let d = diff_catalogs(&[], &cur);
        assert_eq!(d.len(), 1);
        assert!(matches!(d[0].kind, DriftKind::MissingFromExtracted));
    }

    #[test]
    fn drift_kind_extra_is_emitted() {
        let ext = vec![ExtractedExample {
            symbol: "X.y".to_string(),
            signature: "fn".to_string(),
            description: "".to_string(),
            capability: "".to_string(),
            example: "".to_string(),
            see_also: "".to_string(),
            module: "x".to_string(),
        }];
        let d = diff_catalogs(&ext, &[]);
        assert_eq!(d.len(), 1);
        assert!(matches!(d[0].kind, DriftKind::ExtraInExtracted));
    }

    #[test]
    fn render_drift_report_empty_is_empty_string() {
        assert_eq!(render_drift_report(&[]), "");
    }
}
