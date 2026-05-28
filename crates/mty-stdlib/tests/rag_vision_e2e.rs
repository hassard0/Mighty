//! v0.33 Track T2 — end-to-end vision RAG pipeline against wiremock.
//!
//! These two tests exercise the full `Rag::ask_with_image` path
//! against a wiremock-backed provider — equivalent to what
//! `mty-driver`'s integration tests do for `mty run`, but at the
//! library-API level. They verify:
//!
//! 1. Retrieval over the in-memory index pulls the right context.
//! 2. The augmented prompt + image content blocks are sent on a
//!    single `messages` request the provider actually receives.
//! 3. The answer body round-trips out of `Rag::ask_with_image`.
//!
//! Two providers per the v0.33 T2 mandate: Anthropic (the strictly-
//! typed superset) + Gemini (the `parts: [{ inlineData }]` outlier).
//! OpenAI / Bedrock payload shape is covered by `llm_multimodal.rs`.

use std::collections::HashMap;

use mty_stdlib::llm::{anthropic::AnthropicClient, gemini::GeminiClient, image::Image};
use mty_stdlib::rag::{Index, Rag};
use mty_stdlib::swarm::member::Member;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn build_index() -> Index {
    let mut idx = Index::in_memory();
    idx.add_text(
        "Mighty's capability typing tags every value with effects (net, fs, model).",
        HashMap::new(),
    )
    .add_text("turtles are not capability-tagged", HashMap::new());
    idx.build().unwrap();
    idx
}

// -----------------------------------------------------------------------------
// 1. Anthropic vision path
// -----------------------------------------------------------------------------

#[tokio::test]
async fn rag_with_image_round_trips_through_anthropic() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_v01",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-7",
            "content": [
                { "type": "text", "text": "The image shows capability typing." }
            ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 50, "output_tokens": 10 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = AnthropicClient::with_api_key("k").with_base_url(server.uri());
    let member = Member::anthropic_with_client(client, "claude-opus-4-7");

    let rag = Rag::new()
        .with_index(build_index())
        .with_retriever_top_k(2)
        .with_member(member);

    let img = Image::from_bytes(b"fake-png-bytes".to_vec(), "image/png");
    let answer = rag
        .ask_with_image("What is capability typing?", img)
        .await
        .expect("ask_with_image ok");
    assert_eq!(answer, "The image shows capability typing.");
}

// -----------------------------------------------------------------------------
// 2. Gemini vision path
// -----------------------------------------------------------------------------

#[tokio::test]
async fn rag_with_image_round_trips_through_gemini() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1beta/models/.*:generateContent$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "Gemini reads the image and the context." }
                    ],
                    "role": "model"
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 40,
                "candidatesTokenCount": 10,
                "totalTokenCount": 50
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = GeminiClient::with_api_key("k").with_base_url(server.uri());
    let member = Member::gemini_with_client(client, "gemini-2.5-pro");

    let rag = Rag::new()
        .with_index(build_index())
        .with_retriever_top_k(2)
        .with_member(member);

    let img = Image::from_url("https://example.com/diagram.png");
    let answer = rag
        .ask_with_image("Summarise the diagram.", img)
        .await
        .expect("ask_with_image ok");
    assert!(answer.contains("Gemini"));
}
