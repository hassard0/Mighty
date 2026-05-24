//! `std.http` end-to-end: spawn the server on an ephemeral port,
//! issue GETs + POSTs through the client, verify body roundtrip.

use sdust_stdlib::http::{get, post, serve, Handler, Request, Response};
use std::sync::Arc;

fn echo_handler() -> Handler {
    Arc::new(|req: Request| {
        Box::pin(async move {
            let body = format!(
                "{} {}\n{}",
                req.method,
                req.path,
                String::from_utf8_lossy(&req.body)
            )
            .into_bytes();
            Response {
                status: 200,
                body,
                headers: vec![("content-type".into(), "text/plain".into())],
            }
        })
    })
}

#[tokio::test]
async fn get_against_local_server() {
    let (addr, handle) = serve("127.0.0.1:0", echo_handler()).await.unwrap();
    let url = format!("http://{addr}/greet");
    let resp = get(&url).await.unwrap();
    assert_eq!(resp.status, 200);
    assert!(resp.body_str().contains("GET /greet"));
    handle.abort();
}

#[tokio::test]
async fn post_with_body_roundtrips() {
    let (addr, handle) = serve("127.0.0.1:0", echo_handler()).await.unwrap();
    let url = format!("http://{addr}/upload");
    let resp = post(&url, b"payload-bytes".to_vec()).await.unwrap();
    assert_eq!(resp.status, 200);
    let body = resp.body_str();
    assert!(body.contains("POST /upload"), "body was: {body}");
    assert!(body.contains("payload-bytes"), "body was: {body}");
    handle.abort();
}

#[tokio::test]
async fn https_in_v0_2_returns_url_err() {
    let r = get("https://example.com").await;
    assert!(matches!(r, Err(sdust_stdlib::http::HttpErr::Url(_))));
}

#[tokio::test]
async fn server_reports_status_from_handler() {
    let handler: Handler = Arc::new(|_req: Request| {
        Box::pin(async move {
            Response {
                status: 418,
                body: b"i'm a teapot".to_vec(),
                headers: vec![],
            }
        })
    });
    let (addr, handle) = serve("127.0.0.1:0", handler).await.unwrap();
    let url = format!("http://{addr}/");
    let resp = get(&url).await.unwrap();
    assert_eq!(resp.status, 418);
    assert_eq!(resp.body_str(), "i'm a teapot");
    handle.abort();
}
