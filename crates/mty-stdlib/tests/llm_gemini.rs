//! `std.llm.gemini` end-to-end against a wiremock server.
//!
//! Mirrors the `llm_anthropic.rs` shape — wiremock-driven, plain HTTP.
//! Gemini's URL carries the API key as a query parameter; the matcher
//! `path` works because wiremock matches on path independent of query
//! (we assert the query separately via `query_param` where it matters).

use futures_util::StreamExt;
use mty_stdlib::llm::{
    budget::TokenBudget,
    error::LlmError,
    gemini::GeminiClient,
    message::{Message, MessageDelta},
    provider::{CompletionRequest, LlmProvider},
};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> GeminiClient {
    GeminiClient::with_api_key("gem-test").with_base_url(server.uri())
}

#[tokio::test]
async fn gemini_complete_round_trips_with_wiremock() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-pro:generateContent"))
        .and(query_param("key", "gem-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{ "text": "Hello, world!" }]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 12,
                "candidatesTokenCount": 4,
                "totalTokenCount": 16
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let c = client(&server);
    let req = CompletionRequest::new("gemini-2.5-pro", vec![Message::user_text("hi")])
        .with_system("Be brief.");
    let reply = c.complete(req).await.expect("complete ok");
    assert_eq!(reply.text(), "Hello, world!");
}

#[tokio::test]
async fn gemini_tool_use_emits_tool_block() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-pro:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [
                        { "text": "Looking it up." },
                        {
                            "functionCall": {
                                "name": "search",
                                "args": { "q": "rust async" }
                            }
                        }
                    ]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 20,
                "candidatesTokenCount": 12
            }
        })))
        .mount(&server)
        .await;

    let c = client(&server);
    let req = CompletionRequest::new("gemini-2.5-pro", vec![Message::user_text("look it up")]);
    let reply = c.complete(req).await.unwrap();
    let tool_uses = reply.tool_uses();
    assert_eq!(tool_uses.len(), 1);
    assert_eq!(tool_uses[0].name, "search");
    assert_eq!(tool_uses[0].input["q"], "rust async");
}

#[tokio::test]
async fn gemini_rate_limit_returns_typed_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-pro:generateContent"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "30")
                .set_body_string("{\"error\":{\"message\":\"quota\"}}"),
        )
        .mount(&server)
        .await;

    let c = client(&server);
    let req = CompletionRequest::new("gemini-2.5-pro", vec![Message::user_text("hi")]);
    let err = c.complete(req).await.expect_err("must be rate-limited");
    match err {
        LlmError::RateLimit(r) => {
            assert_eq!(r.retry_after_secs, Some(30));
            assert!(r.message.contains("quota"));
        }
        other => panic!("expected RateLimit, got {other:?}"),
    }
}

#[tokio::test]
async fn gemini_complete_records_usage_into_token_budget() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-pro:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "candidates": [{
                "content": { "role": "model", "parts": [{ "text": "ok" }] },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 100,
                "candidatesTokenCount": 200
            }
        })))
        .mount(&server)
        .await;

    let c = client(&server);
    let budget = TokenBudget::new(1000);
    let req = CompletionRequest::new("gemini-2.5-pro", vec![Message::user_text("hi")])
        .with_token_budget(budget.clone());
    c.complete(req).await.unwrap();
    assert_eq!(budget.consumed(), 300);
}

#[tokio::test]
async fn gemini_streaming_parses_provider_events_from_fixture() {
    let server = MockServer::start().await;
    let fixture = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/llm_sse/gemini_text.sse"),
    )
    .unwrap();
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-pro:streamGenerateContent"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(fixture),
        )
        .mount(&server)
        .await;

    let c = client(&server);
    let req = CompletionRequest::new("gemini-2.5-pro", vec![Message::user_text("hi")]);
    let mut stream = c.complete_stream(req).await.unwrap();
    let mut text = String::new();
    let mut saw_done = false;
    while let Some(item) = StreamExt::next(&mut stream).await {
        match item.unwrap() {
            MessageDelta::TextDelta { text: t } => text.push_str(&t),
            MessageDelta::Done { stop_reason } => {
                saw_done = true;
                assert_eq!(stop_reason, "STOP");
            }
            MessageDelta::ToolUseDelta { .. } => {}
        }
    }
    assert_eq!(text, "Hello, world!");
    assert!(saw_done);
}

#[tokio::test]
async fn gemini_budget_exhausted_drops_stream() {
    let server = MockServer::start().await;
    let sse = "\
data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"aaaaaaaaaa\"}]}}]}\n\
\n\
data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"bbbbbbbbbb\"}]}}]}\n\
\n\
data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"cccccccccc\"}]},\"finishReason\":\"STOP\"}]}\n\
\n";
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-pro:streamGenerateContent"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&server)
        .await;

    let c = client(&server);
    let budget = TokenBudget::new(4);
    let req = CompletionRequest::new("gemini-2.5-pro", vec![Message::user_text("hi")])
        .with_token_budget(budget);

    let mut stream = c.complete_stream(req).await.unwrap();
    let mut budget_err_seen = false;
    let mut delta_count = 0;
    while let Some(item) = StreamExt::next(&mut stream).await {
        match item {
            Ok(_) => delta_count += 1,
            Err(LlmError::BudgetExhausted(_)) => {
                budget_err_seen = true;
                break;
            }
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }
    assert!(budget_err_seen);
    assert!(delta_count < 3);
}
