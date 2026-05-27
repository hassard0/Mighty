//! `std.llm.openai` end-to-end against a wiremock server.
//!
//! Mirrors the `llm_anthropic.rs` shape — request shaping, response
//! parsing, budget plumbing, streaming SSE, typed error variants.
//! Plain HTTP because the TLS handshake is covered separately.

use futures_util::StreamExt;
use mty_stdlib::llm::{
    budget::TokenBudget,
    error::LlmError,
    message::{ContentBlock, Message, MessageDelta},
    openai::OpenAiClient,
    provider::{CompletionRequest, LlmProvider},
};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> OpenAiClient {
    OpenAiClient::with_api_key("sk-test").with_base_url(server.uri())
}

#[tokio::test]
async fn openai_complete_round_trips_with_wiremock() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(header("authorization", "Bearer sk-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "resp_01",
            "object": "response",
            "model": "gpt-5",
            "output": [
                {
                    "type": "message",
                    "id": "msg_01",
                    "role": "assistant",
                    "content": [
                        { "type": "output_text", "text": "Hello, world!" }
                    ]
                }
            ],
            "usage": { "input_tokens": 12, "output_tokens": 4 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let c = client(&server);
    let req =
        CompletionRequest::new("gpt-5", vec![Message::user_text("hi")]).with_system("Be brief.");

    let reply = c.complete(req).await.expect("complete ok");
    assert_eq!(reply.text(), "Hello, world!");
}

#[tokio::test]
async fn openai_tool_use_emits_tool_block() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "resp_02",
            "object": "response",
            "model": "gpt-5",
            "output": [
                {
                    "type": "message",
                    "id": "msg_02",
                    "role": "assistant",
                    "content": [
                        { "type": "output_text", "text": "I'll search." }
                    ]
                },
                {
                    "type": "function_call",
                    "call_id": "call_search_01",
                    "name": "search",
                    "arguments": "{\"q\":\"rust async\"}"
                }
            ],
            "usage": { "input_tokens": 20, "output_tokens": 30 }
        })))
        .mount(&server)
        .await;

    let c = client(&server);
    let req = CompletionRequest::new("gpt-5", vec![Message::user_text("look it up")]);
    let reply = c.complete(req).await.unwrap();
    let tool_uses = reply.tool_uses();
    assert_eq!(tool_uses.len(), 1);
    assert_eq!(tool_uses[0].id, "call_search_01");
    assert_eq!(tool_uses[0].name, "search");
    assert_eq!(tool_uses[0].input["q"], "rust async");
    assert!(reply
        .content
        .iter()
        .any(|b| matches!(b, ContentBlock::Text { text } if text == "I'll search.")));
}

#[tokio::test]
async fn openai_rate_limit_returns_typed_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "12")
                .set_body_string("{\"error\":{\"message\":\"slow down\"}}"),
        )
        .mount(&server)
        .await;

    let c = client(&server);
    let req = CompletionRequest::new("gpt-5", vec![Message::user_text("hi")]);
    let err = c.complete(req).await.expect_err("must be rate-limited");
    match err {
        LlmError::RateLimit(r) => {
            assert_eq!(r.retry_after_secs, Some(12));
            assert!(r.message.contains("slow down"));
        }
        other => panic!("expected RateLimit, got {other:?}"),
    }
}

#[tokio::test]
async fn openai_auth_error_surfaces_as_typed_auth_variant() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(401).set_body_string("{}"))
        .mount(&server)
        .await;

    let c = client(&server);
    let req = CompletionRequest::new("gpt-5", vec![Message::user_text("hi")]);
    let err = c.complete(req).await.expect_err("auth fails");
    assert!(matches!(err, LlmError::Auth(_)));
}

#[tokio::test]
async fn openai_complete_records_usage_into_token_budget() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "resp_03",
            "model": "gpt-5",
            "output": [
                {
                    "type": "message",
                    "id": "m",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "ok" }]
                }
            ],
            "usage": { "input_tokens": 100, "output_tokens": 200 }
        })))
        .mount(&server)
        .await;

    let c = client(&server);
    let budget = TokenBudget::new(1000);
    let req = CompletionRequest::new("gpt-5", vec![Message::user_text("hi")])
        .with_token_budget(budget.clone());
    c.complete(req).await.unwrap();
    assert_eq!(budget.consumed(), 300);
}

#[tokio::test]
async fn openai_streaming_parses_sse_chunks() {
    let server = MockServer::start().await;
    let sse = "\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hi\"}\n\
\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\" there\"}\n\
\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\
\n";
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_partial_json(serde_json::json!({"stream": true})))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&server)
        .await;

    let c = client(&server);
    let req = CompletionRequest::new("gpt-5", vec![Message::user_text("hi")]);
    let mut stream = c.complete_stream(req).await.unwrap();
    let mut text = String::new();
    let mut saw_done = false;
    while let Some(item) = StreamExt::next(&mut stream).await {
        match item.unwrap() {
            MessageDelta::TextDelta { text: t } => text.push_str(&t),
            MessageDelta::Done { .. } => saw_done = true,
            MessageDelta::ToolUseDelta { .. } => {}
        }
    }
    assert_eq!(text, "Hi there");
    assert!(saw_done);
}

#[tokio::test]
async fn openai_streaming_parses_provider_events_from_fixture() {
    let server = MockServer::start().await;
    let fixture = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/llm_sse/openai_text.sse"),
    )
    .unwrap();
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(fixture),
        )
        .mount(&server)
        .await;

    let c = client(&server);
    let req = CompletionRequest::new("gpt-5", vec![Message::user_text("hi")]);
    let mut stream = c.complete_stream(req).await.unwrap();
    let mut text = String::new();
    while let Some(item) = StreamExt::next(&mut stream).await {
        if let Ok(MessageDelta::TextDelta { text: t }) = item {
            text.push_str(&t);
        }
    }
    assert_eq!(text, "Hello, world!");
}

#[tokio::test]
async fn openai_budget_exhausted_drops_stream() {
    let server = MockServer::start().await;
    let sse = "\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\"aaaaaaaaaa\"}\n\
\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\"bbbbbbbbbb\"}\n\
\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\"cccccccccc\"}\n\
\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\
\n";
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&server)
        .await;

    let c = client(&server);
    let budget = TokenBudget::new(4);
    let req =
        CompletionRequest::new("gpt-5", vec![Message::user_text("hi")]).with_token_budget(budget);

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
