//! `std.llm.anthropic` end-to-end against a wiremock server.
//!
//! Plain HTTP (not HTTPS) because we're exercising the request
//! shaping + response parsing + budget / rate-limit / streaming
//! plumbing, not the TLS handshake (that's covered separately by
//! `tls_handshake.rs`).
//!
//! Each test spins its own mock server so they run in parallel
//! without cross-talk.

use futures_util::StreamExt;
use mty_stdlib::llm::{
    anthropic::AnthropicClient,
    budget::TokenBudget,
    error::LlmError,
    message::{ContentBlock, Message, MessageDelta},
    provider::{CompletionRequest, LlmProvider},
};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> AnthropicClient {
    AnthropicClient::with_api_key("test-key").with_base_url(server.uri())
}

#[tokio::test]
async fn anthropic_complete_round_trips_with_wiremock() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "test-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_01",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-7",
            "content": [
                { "type": "text", "text": "Hello, world!" }
            ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 12, "output_tokens": 4 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client(&server);
    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("hi")])
        .with_system("Be brief.");

    let reply = client.complete(req).await.expect("complete ok");
    assert_eq!(reply.text(), "Hello, world!");
    // Mock's expect(1) is asserted on drop.
}

#[tokio::test]
async fn anthropic_tool_use_emits_tool_block() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_02",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-7",
            "content": [
                { "type": "text", "text": "I'll search for that." },
                {
                    "type": "tool_use",
                    "id": "toolu_search_01",
                    "name": "search",
                    "input": { "q": "rust async" }
                }
            ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 20, "output_tokens": 30 }
        })))
        .mount(&server)
        .await;

    let client = client(&server);
    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("look it up")]);
    let reply = client.complete(req).await.unwrap();
    let tool_uses = reply.tool_uses();
    assert_eq!(tool_uses.len(), 1);
    assert_eq!(tool_uses[0].id, "toolu_search_01");
    assert_eq!(tool_uses[0].name, "search");
    assert_eq!(tool_uses[0].input["q"], "rust async");
    // The text block also lands.
    assert!(reply
        .content
        .iter()
        .any(|b| matches!(b, ContentBlock::Text { text } if text == "I'll search for that.")));
}

#[tokio::test]
async fn anthropic_rate_limit_returns_typed_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "7")
                .set_body_string("{\"error\":{\"message\":\"slow down\"}}"),
        )
        .mount(&server)
        .await;

    let client = client(&server);
    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("hi")]);
    let err = client
        .complete(req)
        .await
        .expect_err("must be rate-limited");
    match err {
        LlmError::RateLimit(r) => {
            assert_eq!(r.retry_after_secs, Some(7));
            assert!(r.message.contains("slow down"));
        }
        other => panic!("expected RateLimit, got {other:?}"),
    }
}

#[tokio::test]
async fn anthropic_auth_error_surfaces_as_typed_auth_variant() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_string("{\"error\":{\"type\":\"authentication_error\"}}"),
        )
        .mount(&server)
        .await;

    let client = client(&server);
    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("hi")]);
    let err = client.complete(req).await.expect_err("auth fails");
    assert!(matches!(err, LlmError::Auth(_)));
}

#[tokio::test]
async fn anthropic_budget_exhausted_short_circuits_before_request() {
    // No mock is required — the budget pre-check fires *before* the
    // HTTP call goes out. If we did hit the server the test would
    // still pass (wiremock just complains about no matched mock), but
    // by leaving the server empty we prove the pre-check works.
    let server = MockServer::start().await;
    let client = client(&server);

    let budget = TokenBudget::new(10);
    // Pre-consume to put it over the cap.
    let _ = budget.try_consume(20);

    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("hi")])
        .with_token_budget(budget);

    let err = client.complete(req).await.expect_err("budget gate");
    assert!(matches!(err, LlmError::BudgetExhausted(_)));
}

#[tokio::test]
async fn anthropic_complete_records_usage_into_token_budget() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_03",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-7",
            "content": [{ "type": "text", "text": "ok" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 100, "output_tokens": 200 }
        })))
        .mount(&server)
        .await;

    let client = client(&server);
    let budget = TokenBudget::new(1000);
    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("hi")])
        .with_token_budget(budget.clone());
    client.complete(req).await.unwrap();
    assert_eq!(budget.consumed(), 300, "input + output tokens deducted");
}

#[tokio::test]
async fn anthropic_provider_5xx_surfaces_as_provider_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(503).set_body_string("upstream busted"))
        .mount(&server)
        .await;

    let client = client(&server);
    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("hi")]);
    let err = client.complete(req).await.expect_err("provider err");
    match err {
        LlmError::Provider { status, body } => {
            assert_eq!(status, 503);
            assert!(body.contains("busted"));
        }
        other => panic!("expected Provider, got {other:?}"),
    }
}

#[tokio::test]
async fn anthropic_streaming_parses_sse_chunks() {
    let server = MockServer::start().await;
    // A minimal SSE stream: one text delta, then message_stop.
    let sse = "\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" there\"}}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\
\n";
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&server)
        .await;

    let client = client(&server);
    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("hi")]);
    let mut stream = client.complete_stream(req).await.unwrap();
    let mut text = String::new();
    let mut saw_done = false;
    while let Some(item) = stream.next().await {
        match item.unwrap() {
            MessageDelta::TextDelta { text: t } => text.push_str(&t),
            MessageDelta::Done { stop_reason } => {
                saw_done = true;
                assert_eq!(stop_reason, "end_turn");
            }
            MessageDelta::ToolUseDelta { .. } => {}
        }
    }
    assert_eq!(text, "Hi there");
    assert!(saw_done);
}

#[tokio::test]
async fn anthropic_budget_exhausted_drops_stream() {
    let server = MockServer::start().await;
    let sse = "\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"aaaaaaaaaa\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"bbbbbbbbbb\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"cccccccccc\"}}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\
\n";
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&server)
        .await;

    let client = client(&server);
    // Tiny budget — each text delta is ~10 chars (~3 tokens at our
    // chars/4 estimate). A budget of 4 has headroom for exactly
    // one delta before the second crosses the cap.
    let budget = TokenBudget::new(4);
    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("hi")])
        .with_token_budget(budget);

    let mut stream = client.complete_stream(req).await.unwrap();
    let mut budget_err_seen = false;
    let mut delta_count = 0;
    while let Some(item) = stream.next().await {
        match item {
            Ok(_) => delta_count += 1,
            Err(LlmError::BudgetExhausted(_)) => {
                budget_err_seen = true;
                break;
            }
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }
    assert!(budget_err_seen, "expected BudgetExhausted in stream");
    // The stream short-circuits — we shouldn't drain all three deltas.
    assert!(
        delta_count < 3,
        "stream should drop on budget trip; saw {delta_count} deltas"
    );
}
