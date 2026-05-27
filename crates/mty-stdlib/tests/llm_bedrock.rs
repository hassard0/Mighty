//! `std.llm.bedrock` end-to-end against a wiremock server.
//!
//! Two auth shapes covered:
//! - SigV4 signing (the production path). We use SigV4 against
//!   wiremock — wiremock doesn't validate the signature, but it does
//!   prove the `authorization` + `x-amz-date` headers are emitted in
//!   the right shape and that the request body / endpoint routing is
//!   correct.
//! - Bearer-token (Bedrock's newer auth mode). Sidesteps SigV4 entirely.
//!
//! ConverseStream uses AWS's binary event-stream framing rather than
//! SSE; the streaming test feeds the captured `bedrock_text.sse`
//! binary fixture in.

use futures_util::StreamExt;
use mty_stdlib::llm::{
    bedrock::{AwsCredentials, BedrockClient},
    budget::TokenBudget,
    error::LlmError,
    message::{ContentBlock, Message, MessageDelta},
    provider::{CompletionRequest, LlmProvider},
};
use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn bedrock_with_credentials(server: &MockServer) -> BedrockClient {
    BedrockClient::with_credentials(AwsCredentials {
        access_key_id: "AKIDEXAMPLE".into(),
        secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
        session_token: None,
    })
    .with_region("us-east-1")
    .with_base_url(server.uri())
}

fn bedrock_with_bearer(server: &MockServer) -> BedrockClient {
    BedrockClient::with_api_token("bedrock-test-token")
        .with_region("us-east-1")
        .with_base_url(server.uri())
}

#[tokio::test]
async fn bedrock_complete_round_trips_with_wiremock() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/model/anthropic.claude-opus-4-7-v1:0/converse"))
        .and(header_exists("authorization"))
        .and(header_exists("x-amz-date"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "output": {
                "message": {
                    "role": "assistant",
                    "content": [
                        { "text": "Hello, world!" }
                    ]
                }
            },
            "stopReason": "end_turn",
            "usage": {
                "inputTokens": 12,
                "outputTokens": 4,
                "totalTokens": 16
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let c = bedrock_with_credentials(&server);
    assert!(c.signs_with_sigv4(), "test exercises SigV4 path");
    let req = CompletionRequest::new(
        "anthropic.claude-opus-4-7-v1:0",
        vec![Message::user_text("hi")],
    )
    .with_system("Be brief.");
    let reply = c.complete(req).await.expect("complete ok");
    assert_eq!(reply.text(), "Hello, world!");
}

#[tokio::test]
async fn bedrock_tool_use_emits_tool_block() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/model/anthropic.claude-opus-4-7-v1:0/converse"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "output": {
                "message": {
                    "role": "assistant",
                    "content": [
                        { "text": "Searching..." },
                        {
                            "toolUse": {
                                "toolUseId": "tooluse_42",
                                "name": "search",
                                "input": { "q": "rust async" }
                            }
                        }
                    ]
                }
            },
            "stopReason": "tool_use",
            "usage": { "inputTokens": 20, "outputTokens": 30, "totalTokens": 50 }
        })))
        .mount(&server)
        .await;

    let c = bedrock_with_bearer(&server);
    let req = CompletionRequest::new(
        "anthropic.claude-opus-4-7-v1:0",
        vec![Message::user_text("look it up")],
    );
    let reply = c.complete(req).await.unwrap();
    let tool_uses = reply.tool_uses();
    assert_eq!(tool_uses.len(), 1);
    assert_eq!(tool_uses[0].id, "tooluse_42");
    assert_eq!(tool_uses[0].name, "search");
    assert_eq!(tool_uses[0].input["q"], "rust async");
    assert!(reply
        .content
        .iter()
        .any(|b| matches!(b, ContentBlock::Text { text } if text == "Searching...")));
}

#[tokio::test]
async fn bedrock_rate_limit_returns_typed_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/model/anthropic.claude-opus-4-7-v1:0/converse"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "5")
                .set_body_string("{\"message\":\"Throttled\"}"),
        )
        .mount(&server)
        .await;

    let c = bedrock_with_bearer(&server);
    let req = CompletionRequest::new(
        "anthropic.claude-opus-4-7-v1:0",
        vec![Message::user_text("hi")],
    );
    let err = c.complete(req).await.expect_err("must be rate-limited");
    match err {
        LlmError::RateLimit(r) => {
            assert_eq!(r.retry_after_secs, Some(5));
            assert!(r.message.contains("Throttled"));
        }
        other => panic!("expected RateLimit, got {other:?}"),
    }
}

#[tokio::test]
async fn bedrock_complete_records_usage_into_token_budget() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/model/anthropic.claude-opus-4-7-v1:0/converse"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "output": {
                "message": {
                    "role": "assistant",
                    "content": [{ "text": "ok" }]
                }
            },
            "stopReason": "end_turn",
            "usage": { "inputTokens": 100, "outputTokens": 200, "totalTokens": 300 }
        })))
        .mount(&server)
        .await;

    let c = bedrock_with_credentials(&server);
    let budget = TokenBudget::new(1000);
    let req = CompletionRequest::new(
        "anthropic.claude-opus-4-7-v1:0",
        vec![Message::user_text("hi")],
    )
    .with_token_budget(budget.clone());
    c.complete(req).await.unwrap();
    assert_eq!(budget.consumed(), 300);
}

#[tokio::test]
async fn bedrock_streaming_parses_provider_events_from_fixture() {
    let server = MockServer::start().await;
    let fixture_bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/llm_sse/bedrock_text.sse"),
    )
    .unwrap();
    Mock::given(method("POST"))
        .and(path(
            "/model/anthropic.claude-opus-4-7-v1:0/converse-stream",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/vnd.amazon.eventstream")
                .set_body_bytes(fixture_bytes),
        )
        .mount(&server)
        .await;

    let c = bedrock_with_credentials(&server);
    let req = CompletionRequest::new(
        "anthropic.claude-opus-4-7-v1:0",
        vec![Message::user_text("hi")],
    );
    let mut stream = c.complete_stream(req).await.unwrap();
    let mut text = String::new();
    let mut saw_done = false;
    while let Some(item) = StreamExt::next(&mut stream).await {
        match item.unwrap() {
            MessageDelta::TextDelta { text: t } => text.push_str(&t),
            MessageDelta::Done { stop_reason } => {
                saw_done = true;
                assert_eq!(stop_reason, "end_turn");
            }
            MessageDelta::ToolUseDelta { .. } => {}
        }
    }
    assert_eq!(text, "Hello, world!");
    assert!(saw_done);
}

#[tokio::test]
async fn bedrock_budget_exhausted_short_circuits_before_request() {
    let server = MockServer::start().await;
    let c = bedrock_with_credentials(&server);

    let budget = TokenBudget::new(10);
    let _ = budget.try_consume(20);

    let req = CompletionRequest::new(
        "anthropic.claude-opus-4-7-v1:0",
        vec![Message::user_text("hi")],
    )
    .with_token_budget(budget);

    let err = c.complete(req).await.expect_err("budget gate");
    assert!(matches!(err, LlmError::BudgetExhausted(_)));
}
