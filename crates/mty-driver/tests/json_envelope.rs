//! v0.33 T4 — integration test for the structured diagnostic envelope.
//!
//! Drives the full `parse → lower → type+borrow check → envelope`
//! pipeline against the existing taint example
//! (`examples/33_taint_basics.mty`) and asserts the resulting NDJSON
//! is well-formed and shaped as the agent-mode contract requires.

use mty_diagnostics::fix::{to_ndjson, DiagnosticEnvelope};
use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source, type_and_borrow_check};

fn check_to_envelopes(src: &str, source_id: &str) -> Vec<DiagnosticEnvelope> {
    let parsed = parse_source(src.to_string(), source_id.to_string());
    let (pkg, mut diags) = lower(&parsed);
    let lower_errors = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !lower_errors {
        diags.extend(type_and_borrow_check(&pkg));
    }
    let ndjson = to_ndjson(&diags, source_id, src, false);
    if ndjson.is_empty() {
        return vec![];
    }
    ndjson
        .trim_end()
        .split('\n')
        .map(|line| serde_json::from_str(line).expect("each line parses as envelope"))
        .collect()
}

#[test]
fn taint_basics_emits_mt4099_envelope() {
    let path = std::path::Path::new("../../examples/33_taint_basics.mty");
    let src = std::fs::read_to_string(path).expect("example file exists");
    let envs = check_to_envelopes(&src, "examples/33_taint_basics.mty");

    // At least one MT4099 envelope.
    let mt4099 = envs
        .iter()
        .find(|e| e.code == "MT4099")
        .expect("MT4099 envelope present");

    assert_eq!(mt4099.severity, "error");
    assert!(mt4099.span.byte_end >= mt4099.span.byte_start);
    assert!(!mt4099.title.is_empty());
    assert!(!mt4099.prose.is_empty());

    // Marquee fix: three untaint alternatives + confidence ≥ 0.85.
    let fix = mt4099.fix.as_ref().expect("MT4099 carries a fix");
    assert_eq!(fix.kind, "untaint");
    assert_eq!(fix.alternatives.len(), 3);
    assert!(fix.confidence >= 0.85);
    for alt in &fix.alternatives {
        assert!(!alt.diff.is_empty(), "alt `{}` has a diff", alt.label);
        assert!(alt.diff.contains("@@"));
        assert!(alt.confidence >= 0.5);
    }

    // see_also crosslinks to MT4001 + taint docs.
    assert!(mt4099.see_also.iter().any(|s| s == "MT4001"));
    assert!(mt4099
        .see_also
        .iter()
        .any(|s| s == "docs/internals/taint-types.md"));
}

#[test]
fn check_clean_file_emits_zero_envelopes() {
    let src = "package demo\n\nfn main() {\n  log(\"hi\")\n}\n";
    let envs = check_to_envelopes(src, "clean.mty");
    assert!(envs.is_empty(), "clean check produces no envelopes");
}

#[test]
fn ndjson_one_envelope_per_line() {
    let src = "package demo\n\nfn main() {\n  let x = grtng\n  let y = grtng2\n}\n";
    let parsed = parse_source(src.to_string(), "x.mty".to_string());
    let (pkg, mut diags) = lower(&parsed);
    if !diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        diags.extend(type_and_borrow_check(&pkg));
    }
    let ndjson = to_ndjson(&diags, "x.mty", src, false);
    // Either there are diagnostics (each line valid JSON) or there
    // aren't (empty output). The "one valid JSON per line" rule must
    // hold either way.
    for line in ndjson.lines() {
        let _: DiagnosticEnvelope = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("line `{line}` failed to parse: {e}"));
    }
}

#[test]
fn ndjson_include_source_adds_snippet() {
    let src = "package demo\n\nfn main() {\n  let x = mystery\n}\n";
    let parsed = parse_source(src.to_string(), "snip.mty".to_string());
    let (pkg, mut diags) = lower(&parsed);
    if !diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        diags.extend(type_and_borrow_check(&pkg));
    }
    let ndjson = to_ndjson(&diags, "snip.mty", src, true);
    for line in ndjson.lines() {
        let env: DiagnosticEnvelope = serde_json::from_str(line).unwrap();
        // If there's any diagnostic, --include-source populates the snippet.
        assert!(env.source.is_some(), "include_source should attach snippet");
    }
}
