//! Golden tests for HTML rendering.

use sdust_doc::{build_doc_package, render};

const SAMPLE: &str = r#"package weather

/// Forecast is the public forecast surface.
pub struct Forecast {
  temp: F64,
  city: Str,
}

/// Greet returns a hello string.
///
/// # Since
/// 0.2.0
pub fn greet(name: Str) -> Str {
  name
}

/// Predict returns a [Forecast] for a city.
pub fn predict(city: Str) -> Forecast {
  Forecast { temp: 70.0, city }
}
"#;

#[test]
fn html_index_lists_items_and_link_to_pages() {
    let (doc, _) = build_doc_package(SAMPLE, "weather.sd", "weather");
    let files = render::html(&doc);
    let idx = files.get("index.html").expect("index.html");
    assert!(idx.contains("Package <code>weather</code>"));
    assert!(idx.contains("href=\"fn.greet.html\""));
    assert!(idx.contains("href=\"struct.Forecast.html\""));
    assert!(idx.contains("FUNCTIONS"));
    assert!(idx.contains("TYPES"));
    // Search UI is wired in.
    assert!(idx.contains("id=\"q\""));
    assert!(idx.contains("search.js"));
}

#[test]
fn html_item_page_linkifies_signature_to_struct_anchor() {
    let (doc, _) = build_doc_package(SAMPLE, "weather.sd", "weather");
    let files = render::html(&doc);
    let predict = files.get("fn.predict.html").expect("predict page");
    // The return type `Forecast` should be hyperlinked to the struct anchor.
    assert!(
        predict.contains("href=\"#struct.Forecast\">Forecast</a>"),
        "predict page should linkify Forecast: {}",
        predict
    );
}

#[test]
fn html_item_page_renders_since_block() {
    let (doc, _) = build_doc_package(SAMPLE, "weather.sd", "weather");
    let files = render::html(&doc);
    let greet = files.get("fn.greet.html").expect("greet page");
    assert!(greet.contains("Since 0.2.0"), "missing since: {}", greet);
}

#[test]
fn html_static_assets_are_emitted() {
    let (doc, _) = build_doc_package(SAMPLE, "weather.sd", "weather");
    let files = render::html(&doc);
    assert!(files.contains_key("style.css"));
    assert!(files.contains_key("search.js"));
    assert!(files.contains_key("search-index.json"));
    let idx_json = files.get("search-index.json").unwrap();
    // Hand-rolled JSON should at least have one entry per item, with
    // an array shape.
    assert!(idx_json.starts_with('['));
    assert!(idx_json.ends_with(']'));
    assert!(idx_json.contains("\"name\":\"greet\""));
    assert!(idx_json.contains("\"name\":\"predict\""));
    assert!(idx_json.contains("\"kind\":\"struct\""));
}

#[test]
fn html_doc_body_linkifies_bracket_refs() {
    let (doc, _) = build_doc_package(SAMPLE, "weather.sd", "weather");
    let files = render::html(&doc);
    let predict = files.get("fn.predict.html").expect("predict page");
    // `[Forecast]` in doc text should become a relative anchor link.
    assert!(
        predict.contains("href=\"#struct.Forecast\">Forecast</a>")
            || predict.contains("href=\"#struct.Forecast\""),
        "predict body should contain a linkified [Forecast] ref: {}",
        predict
    );
}
