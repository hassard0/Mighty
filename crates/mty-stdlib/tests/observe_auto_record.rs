#![cfg(feature = "observe-sqlite")]
//! End-to-end test: `LlmProvider::complete()` records into the
//! active observe store iff `MTY_OBSERVE=1`.
//!
//! Hits a wiremock'd Anthropic server (same harness as
//! `llm_anthropic.rs`). The point is to pin the **integration**
//! between the provider hook and the storage layer — the unit-test
//! coverage for both halves lives in `observe::{storage,observation}::tests`.

use mty_stdlib::llm::{
    anthropic::AnthropicClient,
    message::Message,
    provider::{CompletionRequest, LlmProvider},
};
use mty_stdlib::observe::storage::{
    install_store, is_recording_enabled, uninstall_store, SqliteStore,
};
use mty_stdlib::observe::with_storage;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The observe store is process-global; serialise tests that touch
/// it so they don't race on `install_store` / `uninstall_store` /
/// the `MTY_OBSERVE` env var.
///
/// Uses `tokio::sync::Mutex` so the guard is await-safe (the wiremock
/// server start is `await`'d, and the std `MutexGuard` lints under
/// `clippy::await_holding_lock`).
async fn store_lock() -> tokio::sync::MutexGuard<'static, ()> {
    use std::sync::OnceLock;
    use tokio::sync::Mutex;
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().await
}

fn anthropic_client(server: &MockServer) -> AnthropicClient {
    AnthropicClient::with_api_key("test-key").with_base_url(server.uri())
}

async fn mount_ok(server: &MockServer, input: u64, output: u64) {
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_obs",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-7",
            "content": [{"type":"text","text":"observed"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": input, "output_tokens": output},
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn complete_records_to_active_store_when_observe_enabled() {
    let _g = store_lock().await;
    std::env::set_var("MTY_OBSERVE", "1");

    uninstall_store();
    let store = SqliteStore::in_memory().unwrap();
    install_store(Box::new(store));

    let server = MockServer::start().await;
    mount_ok(&server, 100, 50).await;

    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("hi")]);
    anthropic_client(&server)
        .complete(req)
        .await
        .expect("complete ok");

    let recorded = with_storage(|s| s.snapshot().unwrap_or_default()).unwrap();
    assert_eq!(recorded.len(), 1, "expected exactly one observation");
    let obs = &recorded[0];
    assert_eq!(obs.provider, "anthropic");
    assert_eq!(obs.model, "claude-opus-4-7");
    assert_eq!(obs.prompt_tokens, 100);
    assert_eq!(obs.completion_tokens, 50);
    // 100 input tokens * 1500 cents/Mtok = 0 cents (integer math),
    // 50 output * 7500 = 0 too. The integer divide is by design:
    // sub-cent calls round to zero; the docs spell this out.
    assert_eq!(obs.cost_cents, 0);
    assert!(
        obs.error_kind.is_none(),
        "success should have no error_kind"
    );

    uninstall_store();
    std::env::remove_var("MTY_OBSERVE");
}

#[tokio::test]
async fn complete_skips_recording_when_observe_disabled() {
    let _g = store_lock().await;
    std::env::remove_var("MTY_OBSERVE");
    assert!(!is_recording_enabled());

    uninstall_store();
    let store = SqliteStore::in_memory().unwrap();
    install_store(Box::new(store));

    let server = MockServer::start().await;
    mount_ok(&server, 50, 25).await;

    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("hi")]);
    anthropic_client(&server)
        .complete(req)
        .await
        .expect("complete ok");

    let recorded = with_storage(|s| s.snapshot().unwrap_or_default()).unwrap();
    assert!(
        recorded.is_empty(),
        "expected no observations when MTY_OBSERVE is off"
    );

    uninstall_store();
}

#[tokio::test]
async fn provider_5xx_still_records_with_error_kind() {
    let _g = store_lock().await;
    std::env::set_var("MTY_OBSERVE", "1");

    uninstall_store();
    let store = SqliteStore::in_memory().unwrap();
    install_store(Box::new(store));

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(500).set_body_string("upstream blew up"))
        .mount(&server)
        .await;

    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("hi")]);
    let res = anthropic_client(&server).complete(req).await;
    assert!(res.is_err(), "5xx should propagate as error");

    let recorded = with_storage(|s| s.snapshot().unwrap_or_default()).unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].error_kind.as_deref(), Some("provider"));
    assert_eq!(recorded[0].prompt_tokens, 0);

    uninstall_store();
    std::env::remove_var("MTY_OBSERVE");
}

#[tokio::test]
async fn rate_limit_records_with_rate_limit_error_kind() {
    let _g = store_lock().await;
    std::env::set_var("MTY_OBSERVE", "1");

    uninstall_store();
    let store = SqliteStore::in_memory().unwrap();
    install_store(Box::new(store));

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "5")
                .set_body_string("slow down"),
        )
        .mount(&server)
        .await;

    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("hi")]);
    let _ = anthropic_client(&server).complete(req).await;

    let recorded = with_storage(|s| s.snapshot().unwrap_or_default()).unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].error_kind.as_deref(), Some("rate_limit"));

    uninstall_store();
    std::env::remove_var("MTY_OBSERVE");
}
