//! v0.29 Track E — per-provider `*_BASE_URL` env vars for mock-LLM tests.
//!
//! Each LLM provider's `from_env()` now consults:
//!   1. `<PROVIDER>_BASE_URL` (e.g. `ANTHROPIC_BASE_URL`)
//!   2. `MTY_LLM_BASE_URL` (universal fallback)
//!   3. Hard-coded production URL (last resort)
//!
//! These tests pin the resolution order for each provider so future
//! refactors can't silently regress what was a v0.27 / v0.28 pain point
//! (the swarm-review demo had no clean way to redirect a single
//! provider at a wiremock listener — the only option was to thread a
//! base-url override through every call site).
//!
//! NOTE: env vars are process-global, so we serialise the tests
//! through a `Mutex` to avoid one test's `MTY_LLM_BASE_URL=X` leaking
//! into the next test's resolution.

use mty_stdlib::llm::{
    anthropic::AnthropicClient, bedrock::BedrockClient, gemini::GeminiClient, openai::OpenAiClient,
};
use std::sync::Mutex;

// Serialise env mutation across tests in this file. Cargo runs test
// functions in parallel inside the same process by default; without a
// guard, one test's `set_var` would race with another's `from_env`.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Save/restore guard: snapshots the env vars we touch and restores
/// them on drop, so tests don't bleed state into each other (or into
/// the developer's shell when running `cargo test -- --nocapture` from
/// a real terminal).
struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn snapshot(keys: &[&'static str]) -> Self {
        let saved = keys.iter().map(|k| (*k, std::env::var(k).ok())).collect();
        // Clear before the test runs so a leaked outer value can't
        // confuse the assertions.
        for k in keys {
            std::env::remove_var(k);
        }
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, v) in self.saved.drain(..) {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }
}

// --- Anthropic ----------------------------------------------------

#[test]
fn anthropic_provider_specific_base_url_wins() {
    let _g = ENV_LOCK.lock().unwrap();
    let _e = EnvGuard::snapshot(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "MTY_LLM_BASE_URL",
    ]);
    std::env::set_var("ANTHROPIC_API_KEY", "test-key");
    std::env::set_var("ANTHROPIC_BASE_URL", "http://127.0.0.1:9101/");
    std::env::set_var("MTY_LLM_BASE_URL", "http://127.0.0.1:9999/");

    let client = AnthropicClient::from_env().expect("from_env ok");
    assert_eq!(client.base_url(), "http://127.0.0.1:9101/");
}

#[test]
fn anthropic_falls_back_to_universal_var() {
    let _g = ENV_LOCK.lock().unwrap();
    let _e = EnvGuard::snapshot(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "MTY_LLM_BASE_URL",
    ]);
    std::env::set_var("ANTHROPIC_API_KEY", "test-key");
    // No ANTHROPIC_BASE_URL — universal fallback should win.
    std::env::set_var("MTY_LLM_BASE_URL", "http://127.0.0.1:9200/");

    let client = AnthropicClient::from_env().expect("from_env ok");
    assert_eq!(client.base_url(), "http://127.0.0.1:9200/");
}

#[test]
fn anthropic_falls_back_to_hardcoded_when_nothing_set() {
    let _g = ENV_LOCK.lock().unwrap();
    let _e = EnvGuard::snapshot(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "MTY_LLM_BASE_URL",
    ]);
    std::env::set_var("ANTHROPIC_API_KEY", "test-key");
    // Neither env var set — hard-coded prod URL.

    let client = AnthropicClient::from_env().expect("from_env ok");
    assert_eq!(client.base_url(), "https://api.anthropic.com");
}

#[test]
fn anthropic_empty_provider_var_falls_through() {
    // An empty `ANTHROPIC_BASE_URL=` (the common shell shape on a
    // stray `export ANTHROPIC_BASE_URL=`) must NOT silently redirect
    // every request at `""/v1/messages`. We fall through to the
    // universal var, then to the hard-coded URL.
    let _g = ENV_LOCK.lock().unwrap();
    let _e = EnvGuard::snapshot(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "MTY_LLM_BASE_URL",
    ]);
    std::env::set_var("ANTHROPIC_API_KEY", "test-key");
    std::env::set_var("ANTHROPIC_BASE_URL", "");
    std::env::set_var("MTY_LLM_BASE_URL", "http://127.0.0.1:9300/");

    let client = AnthropicClient::from_env().expect("from_env ok");
    assert_eq!(client.base_url(), "http://127.0.0.1:9300/");
}

// --- OpenAI -------------------------------------------------------

#[test]
fn openai_provider_specific_base_url_wins() {
    let _g = ENV_LOCK.lock().unwrap();
    let _e = EnvGuard::snapshot(&["OPENAI_API_KEY", "OPENAI_BASE_URL", "MTY_LLM_BASE_URL"]);
    std::env::set_var("OPENAI_API_KEY", "sk-test");
    std::env::set_var("OPENAI_BASE_URL", "http://127.0.0.1:9401/");
    std::env::set_var("MTY_LLM_BASE_URL", "http://127.0.0.1:9999/");

    let client = OpenAiClient::from_env().expect("from_env ok");
    assert_eq!(client.base_url(), "http://127.0.0.1:9401/");
}

#[test]
fn openai_falls_back_to_universal_var() {
    let _g = ENV_LOCK.lock().unwrap();
    let _e = EnvGuard::snapshot(&["OPENAI_API_KEY", "OPENAI_BASE_URL", "MTY_LLM_BASE_URL"]);
    std::env::set_var("OPENAI_API_KEY", "sk-test");
    std::env::set_var("MTY_LLM_BASE_URL", "http://127.0.0.1:9500/");

    let client = OpenAiClient::from_env().expect("from_env ok");
    assert_eq!(client.base_url(), "http://127.0.0.1:9500/");
}

#[test]
fn openai_falls_back_to_hardcoded_when_nothing_set() {
    let _g = ENV_LOCK.lock().unwrap();
    let _e = EnvGuard::snapshot(&["OPENAI_API_KEY", "OPENAI_BASE_URL", "MTY_LLM_BASE_URL"]);
    std::env::set_var("OPENAI_API_KEY", "sk-test");

    let client = OpenAiClient::from_env().expect("from_env ok");
    assert_eq!(client.base_url(), "https://api.openai.com");
}

// --- Gemini -------------------------------------------------------

#[test]
fn gemini_provider_specific_base_url_wins() {
    let _g = ENV_LOCK.lock().unwrap();
    let _e = EnvGuard::snapshot(&[
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GEMINI_BASE_URL",
        "MTY_LLM_BASE_URL",
    ]);
    std::env::set_var("GEMINI_API_KEY", "g-test");
    std::env::set_var("GEMINI_BASE_URL", "http://127.0.0.1:9601/");
    std::env::set_var("MTY_LLM_BASE_URL", "http://127.0.0.1:9999/");

    let client = GeminiClient::from_env().expect("from_env ok");
    assert_eq!(client.base_url(), "http://127.0.0.1:9601/");
}

#[test]
fn gemini_falls_back_to_universal_var() {
    let _g = ENV_LOCK.lock().unwrap();
    let _e = EnvGuard::snapshot(&[
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GEMINI_BASE_URL",
        "MTY_LLM_BASE_URL",
    ]);
    std::env::set_var("GEMINI_API_KEY", "g-test");
    std::env::set_var("MTY_LLM_BASE_URL", "http://127.0.0.1:9700/");

    let client = GeminiClient::from_env().expect("from_env ok");
    assert_eq!(client.base_url(), "http://127.0.0.1:9700/");
}

#[test]
fn gemini_falls_back_to_hardcoded_when_nothing_set() {
    let _g = ENV_LOCK.lock().unwrap();
    let _e = EnvGuard::snapshot(&[
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GEMINI_BASE_URL",
        "MTY_LLM_BASE_URL",
    ]);
    std::env::set_var("GEMINI_API_KEY", "g-test");

    let client = GeminiClient::from_env().expect("from_env ok");
    assert_eq!(
        client.base_url(),
        "https://generativelanguage.googleapis.com"
    );
}

// --- Bedrock ------------------------------------------------------

#[test]
fn bedrock_provider_specific_base_url_wins() {
    let _g = ENV_LOCK.lock().unwrap();
    let _e = EnvGuard::snapshot(&[
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "AWS_BEDROCK_API_TOKEN",
        "AWS_REGION",
        "BEDROCK_BASE_URL",
        "MTY_LLM_BASE_URL",
    ]);
    std::env::set_var("AWS_BEDROCK_API_TOKEN", "bt-test");
    std::env::set_var("BEDROCK_BASE_URL", "http://127.0.0.1:9801/");
    std::env::set_var("MTY_LLM_BASE_URL", "http://127.0.0.1:9999/");

    let client = BedrockClient::from_env().expect("from_env ok");
    assert_eq!(client.base_url(), "http://127.0.0.1:9801/");
}

#[test]
fn bedrock_falls_back_to_universal_var() {
    let _g = ENV_LOCK.lock().unwrap();
    let _e = EnvGuard::snapshot(&[
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "AWS_BEDROCK_API_TOKEN",
        "AWS_REGION",
        "BEDROCK_BASE_URL",
        "MTY_LLM_BASE_URL",
    ]);
    std::env::set_var("AWS_BEDROCK_API_TOKEN", "bt-test");
    std::env::set_var("MTY_LLM_BASE_URL", "http://127.0.0.1:9900/");

    let client = BedrockClient::from_env().expect("from_env ok");
    assert_eq!(client.base_url(), "http://127.0.0.1:9900/");
}

#[test]
fn bedrock_falls_back_to_region_derived_when_nothing_set() {
    let _g = ENV_LOCK.lock().unwrap();
    let _e = EnvGuard::snapshot(&[
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "AWS_BEDROCK_API_TOKEN",
        "AWS_REGION",
        "BEDROCK_BASE_URL",
        "MTY_LLM_BASE_URL",
    ]);
    std::env::set_var("AWS_BEDROCK_API_TOKEN", "bt-test");
    std::env::set_var("AWS_REGION", "eu-west-1");

    let client = BedrockClient::from_env().expect("from_env ok");
    // No override; URL is composed from AWS_REGION.
    assert_eq!(
        client.base_url(),
        "https://bedrock-runtime.eu-west-1.amazonaws.com"
    );
}

// --- Cross-provider universal fallback ----------------------------

#[test]
fn universal_var_redirects_all_providers_at_once() {
    // The motivating use case: a single mock LLM server that wants to
    // serve every provider's request. One env var, three clients,
    // every client points at the same base URL.
    let _g = ENV_LOCK.lock().unwrap();
    let _e = EnvGuard::snapshot(&[
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "AWS_BEDROCK_API_TOKEN",
        "ANTHROPIC_BASE_URL",
        "OPENAI_BASE_URL",
        "GEMINI_BASE_URL",
        "BEDROCK_BASE_URL",
        "MTY_LLM_BASE_URL",
    ]);
    std::env::set_var("ANTHROPIC_API_KEY", "k");
    std::env::set_var("OPENAI_API_KEY", "k");
    std::env::set_var("GEMINI_API_KEY", "k");
    std::env::set_var("AWS_BEDROCK_API_TOKEN", "k");
    std::env::set_var("MTY_LLM_BASE_URL", "http://127.0.0.1:9000/");

    assert_eq!(
        AnthropicClient::from_env().unwrap().base_url(),
        "http://127.0.0.1:9000/"
    );
    assert_eq!(
        OpenAiClient::from_env().unwrap().base_url(),
        "http://127.0.0.1:9000/"
    );
    assert_eq!(
        GeminiClient::from_env().unwrap().base_url(),
        "http://127.0.0.1:9000/"
    );
    assert_eq!(
        BedrockClient::from_env().unwrap().base_url(),
        "http://127.0.0.1:9000/"
    );
}

#[tokio::test]
async fn anthropic_from_env_honors_url_against_wiremock() {
    // End-to-end shape: spin a real wiremock server, point Anthropic
    // at it via `ANTHROPIC_BASE_URL`, verify the request lands on the
    // mock (not on the prod URL). This is the canonical mock-LLM
    // pattern that v0.29 Track E was carved out to support.
    use mty_stdlib::llm::{
        message::Message,
        provider::{CompletionRequest, LlmProvider},
    };
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Start the mock server FIRST (no env mutation yet). Mounting +
    // network setup happens off-lock so the env-var critical section
    // is sync-only — we never hold a std::sync::Mutex across an await.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_01",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-7",
            "content": [{ "type": "text", "text": "ok" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    // Now take the env-mutation lock + build the client. We hold the
    // lock just long enough to (a) snapshot existing env, (b) install
    // the test env, (c) construct the client (which reads the env).
    // The client clones its base URL into its own field, so releasing
    // the lock + restoring env afterwards doesn't undo the redirect.
    let client = {
        let _g = ENV_LOCK.lock().unwrap();
        let _e = EnvGuard::snapshot(&[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "MTY_LLM_BASE_URL",
        ]);
        std::env::set_var("ANTHROPIC_API_KEY", "test-key");
        std::env::set_var("ANTHROPIC_BASE_URL", server.uri());
        let c = AnthropicClient::from_env().expect("from_env ok");
        assert_eq!(c.base_url(), server.uri());
        c
        // _g + _e drop here, restoring env + releasing the lock BEFORE
        // we start awaiting the HTTP round-trip below.
    };

    let req = CompletionRequest::new("claude-opus-4-7", vec![Message::user_text("hi")]);
    let reply = client.complete(req).await.expect("round-trip ok");
    assert_eq!(reply.text(), "ok");
    // wiremock asserts expect(1) on drop — proves the request landed
    // on the mock and NOT on api.anthropic.com.
}
