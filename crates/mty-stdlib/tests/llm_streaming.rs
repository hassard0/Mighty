//! SSE-parser unit tests against captured Anthropic fixture files.
//!
//! These tests drive the pure-function parser directly — no network,
//! no async, no wiremock. They pin the per-event-type projection
//! onto [`MessageDelta`] so changes in the parser are caught
//! without spinning a runtime.

use mty_stdlib::llm::message::MessageDelta;
use mty_stdlib::llm::streaming::parse_anthropic_sse;
use std::fs;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/llm_sse");
    p.push(name);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("read fixture {}: {e}", p.display()))
}

#[test]
fn text_only_fixture_concatenates_to_full_message() {
    let body = fixture("text_only.sse");
    let (deltas, tail) = parse_anthropic_sse(&body);
    let text: String = deltas
        .iter()
        .filter_map(|d| match d {
            MessageDelta::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Hello, world!");
    assert!(tail.is_empty(), "fixture is fully framed");
    // Terminal Done event must land with end_turn reason.
    let done = deltas
        .iter()
        .find_map(|d| match d {
            MessageDelta::Done { stop_reason } => Some(stop_reason.as_str()),
            _ => None,
        })
        .expect("Done event present");
    assert_eq!(done, "end_turn");
}

#[test]
fn tool_use_fixture_stitches_input_json_into_one_payload() {
    let body = fixture("tool_use.sse");
    let (deltas, _) = parse_anthropic_sse(&body);
    let mut joined = String::new();
    let mut tool_id = String::new();
    let mut tool_name = String::new();
    for d in &deltas {
        if let MessageDelta::ToolUseDelta {
            id,
            name,
            input_partial,
        } = d
        {
            tool_id = id.clone();
            tool_name = name.clone();
            joined.push_str(input_partial);
        }
    }
    assert_eq!(tool_id, "toolu_search_99");
    assert_eq!(tool_name, "search");
    // The stitched JSON should be valid + carry the search query.
    let parsed: serde_json::Value =
        serde_json::from_str(&joined).expect("valid JSON when stitched");
    assert_eq!(parsed["q"], "rust async");

    // The preceding text block still lands too.
    let text: String = deltas
        .iter()
        .filter_map(|d| match d {
            MessageDelta::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Searching...");

    // Stop reason is tool_use, not end_turn.
    let done = deltas
        .iter()
        .find_map(|d| match d {
            MessageDelta::Done { stop_reason } => Some(stop_reason.as_str()),
            _ => None,
        })
        .unwrap();
    assert_eq!(done, "tool_use");
}

#[test]
fn multi_paragraph_fixture_preserves_newlines_inside_deltas() {
    let body = fixture("multi_paragraph.sse");
    let (deltas, _) = parse_anthropic_sse(&body);
    let text: String = deltas
        .iter()
        .filter_map(|d| match d {
            MessageDelta::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(text.contains("Paragraph one.\n\n"));
    assert!(text.contains("Paragraph two.\n\n"));
    assert!(text.contains("Paragraph three."));
    let done = deltas
        .iter()
        .find_map(|d| match d {
            MessageDelta::Done { stop_reason } => Some(stop_reason.as_str()),
            _ => None,
        })
        .unwrap();
    assert_eq!(done, "max_tokens");
}

#[test]
fn unknown_event_types_are_dropped_without_breaking_parse() {
    let body = fixture("unknown_events.sse");
    let (deltas, _) = parse_anthropic_sse(&body);
    // Despite the unknown `ping` + `future_event_type_we_havent_seen_yet`
    // events, we still extract the one text delta + terminal Done.
    let text: String = deltas
        .iter()
        .filter_map(|d| match d {
            MessageDelta::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "ok");
    assert!(deltas
        .iter()
        .any(|d| matches!(d, MessageDelta::Done { .. })));
}

#[test]
fn parser_handles_chunked_feed_by_carrying_tail_across_calls() {
    // Simulate the wire splitting a fixture mid-event. The tail from
    // call 1 must round-trip into call 2 so the truncated event
    // eventually parses.
    let body = fixture("text_only.sse");
    // Cut in the middle of the third `content_block_delta` event.
    let cut = body
        .find("\"world!\"")
        .expect("fixture contains literal `world!` text");
    let (a, b) = body.split_at(cut);

    let (deltas_a, tail_a) = parse_anthropic_sse(a);
    let combined = format!("{tail_a}{b}");
    let (deltas_b, _tail_b) = parse_anthropic_sse(&combined);

    let mut text = String::new();
    for d in deltas_a.iter().chain(deltas_b.iter()) {
        if let MessageDelta::TextDelta { text: t } = d {
            text.push_str(t);
        }
    }
    assert_eq!(text, "Hello, world!");
}
