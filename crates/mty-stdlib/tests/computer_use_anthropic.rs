//! `std.computer` end-to-end against a wiremock'd Anthropic Messages
//! endpoint.
//!
//! v0.30 Track C. Mirrors the shape of `llm_anthropic.rs`: each test
//! spins its own [`MockServer`] and asserts the wire shape the
//! [`AnthropicClient::ask_with_computer`] + [`Dispatcher`] surface
//! emits.
//!
//! These tests do NOT exercise real screenshots / clicks; the
//! dispatcher's [`MockScreen`] + [`MockMouse`] / [`MockKeyboard`]
//! backends are wired in so the assertions are on the request body +
//! the recorded action log only.

use mty_stdlib::computer::dispatcher::{Dispatcher, MAX_TURNS};
use mty_stdlib::computer::input::{MockKeyboard, MockMouse, MouseButton};
use mty_stdlib::computer::sandbox::ComputerCap;
use mty_stdlib::computer::screen::MockScreen;
use mty_stdlib::computer::ComputerError;
use mty_stdlib::llm::anthropic::AnthropicClient;
use mty_stdlib::llm::message::Message;
use mty_stdlib::llm::provider::CompletionRequest;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn client(server: &MockServer) -> AnthropicClient {
    AnthropicClient::with_api_key("test-key").with_base_url(server.uri())
}

#[tokio::test]
async fn ask_with_computer_sends_computer_20241022_tool_spec() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "test-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .and(header("anthropic-beta", "computer-use-2024-10-22"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_01",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-7",
            "content": [{ "type": "text", "text": "took a look" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 5, "output_tokens": 5 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client(&server);
    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("look")]);
    let _ = client
        .ask_with_computer(req, (1280, 800))
        .await
        .expect("ask_with_computer ok");
    // Mock's expect(1) is asserted on drop.
}

#[tokio::test]
async fn ask_with_computer_request_body_carries_computer_tool() {
    let server = MockServer::start().await;
    // Use a generic responder that captures the request body for inspection.
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            let tools = body["tools"].as_array().expect("tools array");
            assert_eq!(tools.len(), 1, "expected exactly one tool");
            assert_eq!(tools[0]["type"], "computer_20241022");
            assert_eq!(tools[0]["name"], "computer");
            assert_eq!(tools[0]["display_width_px"], 1024);
            assert_eq!(tools[0]["display_height_px"], 768);
            assert_eq!(tools[0]["display_number"], 1);
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "msg_02",
                "type": "message",
                "role": "assistant",
                "model": "claude-opus-4-7",
                "content": [{ "type": "text", "text": "ok" }],
                "stop_reason": "end_turn",
                "usage": { "input_tokens": 1, "output_tokens": 1 }
            }))
        })
        .expect(1)
        .mount(&server)
        .await;

    let client = client(&server);
    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("look")]);
    let _ = client.ask_with_computer(req, (1024, 768)).await.unwrap();
}

#[tokio::test]
async fn dispatcher_loop_against_canned_anthropic_replies_terminates_on_done() {
    let server = MockServer::start().await;
    // Reply #1: take a screenshot.
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_a",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-7",
            "content": [
                {
                    "type": "tool_use",
                    "id": "tu_a",
                    "name": "computer",
                    "input": { "action": "screenshot" }
                }
            ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 3, "output_tokens": 3 }
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    // Reply #2: emit `done` to terminate the loop.
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_b",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-7",
            "content": [
                {
                    "type": "tool_use",
                    "id": "tu_b",
                    "name": "computer",
                    "input": { "action": "done", "summary": "done after screenshot" }
                }
            ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 3, "output_tokens": 3 }
        })))
        .mount(&server)
        .await;

    let client = client(&server);
    let cap = ComputerCap::screen_and_input();
    let mouse = MockMouse::default();
    let kb = MockKeyboard::default();
    let dispatcher = Dispatcher::new(client, cap)
        .with_screen(MockScreen::default())
        .with_mouse(mouse.clone())
        .with_keyboard(kb.clone());
    // NOTE: Dispatcher uses `complete()` (no `anthropic-beta` header)
    // because the dispatcher's tool spec is wired in via the regular
    // `tools` array, not via the dedicated `ask_with_computer` path
    // (the v0.30 wire spec is identical when the model is told the
    // tool name + shape directly).
    let out = dispatcher.run("verify").await.expect("run ok");
    assert_eq!(out, "done after screenshot");
    // Mouse / kb stayed untouched — no click / type was requested.
    assert!(mouse.is_empty());
    assert!(kb.is_empty());
}

#[tokio::test]
async fn dispatcher_loop_executes_click_then_done() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "m1",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-7",
            "content": [{
                "type": "tool_use",
                "id": "tu_click",
                "name": "computer",
                "input": { "action": "left_click", "x": 50, "y": 60 }
            }],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "m2",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-7",
            "content": [{
                "type": "tool_use",
                "id": "tu_done",
                "name": "computer",
                "input": { "action": "done", "summary": "clicked" }
            }],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        })))
        .mount(&server)
        .await;

    let client = client(&server);
    let cap = ComputerCap::screen_and_input().with_bounds(0, 0, 1280, 800);
    let mouse = MockMouse::default();
    let dispatcher = Dispatcher::new(client, cap).with_mouse(mouse.clone());
    let out = dispatcher.run("click stuff").await.unwrap();
    assert_eq!(out, "clicked");
    let events = mouse.events();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        mty_stdlib::computer::input::MouseEvent::Click {
            x: 50,
            y: 60,
            button: MouseButton::Left,
            count: 1
        }
    ));
}

#[tokio::test]
async fn dispatcher_sandbox_rejection_short_circuits_loop() {
    // The first reply asks for a click outside the cap's bounds. The
    // dispatcher must surface a SandboxViolation and the mouse log
    // must stay empty.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "x",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-7",
            "content": [{
                "type": "tool_use",
                "id": "tu",
                "name": "computer",
                "input": { "action": "left_click", "x": 9999, "y": 9999 }
            }],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        })))
        .mount(&server)
        .await;

    let client = client(&server);
    let cap = ComputerCap::screen_and_input().with_bounds(0, 0, 100, 100);
    let mouse = MockMouse::default();
    let dispatcher = Dispatcher::new(client, cap).with_mouse(mouse.clone());
    let err = dispatcher.run("escape").await.unwrap_err();
    assert!(matches!(err, ComputerError::SandboxViolation(_)));
    assert!(mouse.is_empty(), "mouse must NOT have been touched");
}

#[tokio::test]
async fn max_turns_constant_is_30() {
    // Sanity check on the public constant: it's part of the API contract.
    assert_eq!(MAX_TURNS, 30);
}
