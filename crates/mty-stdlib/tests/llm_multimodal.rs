//! v0.33 Track T2 — multi-modal payload shape across the 4 providers.
//!
//! Pin the wire shape for [`ContentBlock::Image`] on every provider so
//! a future refactor can't silently drop image input from one of the
//! four. Each provider has its own JSON schema for the image content
//! block; the assertions here are the canonical reference.

use mty_stdlib::llm::{
    anthropic::AnthropicClient,
    bedrock::{AwsCredentials, BedrockClient},
    gemini::GeminiClient,
    image::Image,
    message::{ContentBlock, Message, Role},
    openai::OpenAiClient,
    provider::CompletionRequest,
};

fn image_request(image: Image) -> CompletionRequest {
    let content = vec![
        ContentBlock::Image {
            source: image.to_source().expect("image source"),
        },
        ContentBlock::text("describe the image"),
    ];
    CompletionRequest::new(
        "model-under-test",
        vec![Message {
            role: Role::User,
            content,
        }],
    )
}

// -----------------------------------------------------------------------------
// Anthropic
// -----------------------------------------------------------------------------

#[test]
fn anthropic_payload_carries_base64_image_block() {
    let client = AnthropicClient::with_api_key("k");
    let img = Image::from_bytes(b"foobar".to_vec(), "image/png");
    let body = client.build_body(&image_request(img));
    // body.messages[0].content[0] must be the image block.
    let block = &body["messages"][0]["content"][0];
    assert_eq!(block["type"], "image");
    assert_eq!(block["source"]["type"], "base64");
    assert_eq!(block["source"]["media_type"], "image/png");
    // base64('foobar') = 'Zm9vYmFy'.
    assert_eq!(block["source"]["data"], "Zm9vYmFy");
    // The text block survives intact alongside.
    let text_block = &body["messages"][0]["content"][1];
    assert_eq!(text_block["type"], "text");
    assert_eq!(text_block["text"], "describe the image");
}

#[test]
fn anthropic_payload_carries_url_image_block() {
    let client = AnthropicClient::with_api_key("k");
    let img = Image::from_url("https://example.com/x.png");
    let body = client.build_body(&image_request(img));
    let block = &body["messages"][0]["content"][0];
    assert_eq!(block["type"], "image");
    assert_eq!(block["source"]["type"], "url");
    assert_eq!(block["source"]["url"], "https://example.com/x.png");
}

// -----------------------------------------------------------------------------
// OpenAI
// -----------------------------------------------------------------------------

#[test]
fn openai_payload_carries_input_image_with_data_url() {
    let client = OpenAiClient::with_api_key("k");
    let img = Image::from_bytes(b"foobar".to_vec(), "image/jpeg");
    let body = client.build_body(&image_request(img));
    let block = &body["input"][0]["content"][0];
    assert_eq!(block["type"], "input_image");
    let url = block["image_url"].as_str().unwrap();
    assert!(url.starts_with("data:image/jpeg;base64,"));
    assert!(url.contains("Zm9vYmFy"));
}

#[test]
fn openai_payload_carries_passthrough_url() {
    let client = OpenAiClient::with_api_key("k");
    let img = Image::from_url("https://example.com/y.png");
    let body = client.build_body(&image_request(img));
    let block = &body["input"][0]["content"][0];
    assert_eq!(block["type"], "input_image");
    assert_eq!(block["image_url"], "https://example.com/y.png");
}

// -----------------------------------------------------------------------------
// Gemini
// -----------------------------------------------------------------------------

#[test]
fn gemini_payload_emits_inline_data_block() {
    let client = GeminiClient::with_api_key("k");
    let img = Image::from_bytes(b"foobar".to_vec(), "image/webp");
    let body = client.build_body(&image_request(img));
    let part = &body["contents"][0]["parts"][0];
    assert_eq!(part["inlineData"]["mimeType"], "image/webp");
    assert_eq!(part["inlineData"]["data"], "Zm9vYmFy");
}

#[test]
fn gemini_payload_emits_file_data_for_url() {
    let client = GeminiClient::with_api_key("k");
    let img = Image::from_url("https://example.com/z.png");
    let body = client.build_body(&image_request(img));
    let part = &body["contents"][0]["parts"][0];
    assert_eq!(part["fileData"]["fileUri"], "https://example.com/z.png");
}

// -----------------------------------------------------------------------------
// Bedrock (Anthropic on Bedrock — Converse API)
// -----------------------------------------------------------------------------

#[test]
fn bedrock_payload_emits_image_block_with_format_and_bytes() {
    let client = BedrockClient::with_credentials(AwsCredentials {
        access_key_id: "ak".into(),
        secret_access_key: "sk".into(),
        session_token: None,
    });
    let img = Image::from_bytes(b"foobar".to_vec(), "image/png");
    let body = client.build_body(&image_request(img));
    let block = &body["messages"][0]["content"][0];
    assert_eq!(block["image"]["format"], "png");
    assert_eq!(block["image"]["source"]["bytes"], "Zm9vYmFy");
}

#[test]
fn bedrock_payload_falls_back_to_text_for_url_images() {
    // Bedrock's Converse API doesn't accept URL sources directly. The
    // provider emits a stand-in text block referencing the URL so the
    // request doesn't 400 silently — the test guards that fallback.
    let client = BedrockClient::with_credentials(AwsCredentials {
        access_key_id: "ak".into(),
        secret_access_key: "sk".into(),
        session_token: None,
    });
    let img = Image::from_url("https://example.com/a.png");
    let body = client.build_body(&image_request(img));
    let block = &body["messages"][0]["content"][0];
    let text = block["text"].as_str().unwrap_or("");
    assert!(text.contains("https://example.com/a.png"));
}
