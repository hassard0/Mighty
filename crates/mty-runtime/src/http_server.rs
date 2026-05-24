//! v0.8 loose-end 2/4 — agent-backed HTTP server bridge.
//!
//! The stdlib crate (`mty-stdlib::http_server`) owns the process-wide
//! HTTP listener + a single installable `AgentDispatch` callback. This
//! module is the runtime-side companion: it builds an `AgentDispatch`
//! closure that funnels every incoming request into a real agent's
//! mailbox as an `ask` message, blocks on the reply, and translates the
//! reply value back into an HTTP response.
//!
//! ## Wiring
//!
//! ```ignore
//! use mty_runtime::http_server::install_agent_dispatcher;
//!
//! let rt = RuntimeBuilder::new().build(prog);
//! let h = rt.spawn_agent("Searcher", vec![]).await?;
//! install_agent_dispatcher(&rt, h.clone(), "Request");
//! // From here, `std.http.serve("127.0.0.1:0")` from Mighty source
//! // binds a real socket and routes every request through `h` as
//! // `h?Request(...)`.
//! ```
//!
//! ## Request → ask payload shape
//!
//! The bridge synthesises a Mighty-side ask of the form
//! `agent?MessageName(req_json)` where `req_json` is a
//! deterministically-encoded JSON string of the request:
//!
//! ```json
//! {"method":"GET","path":"/health","body":""}
//! ```
//!
//! The handler returns either a plain `Str` (treated as the response
//! body with HTTP 200) or a JSON-encoded `{"status": 200, "body":
//! "..."}` object.
//!
//! ## Why a sync ask under a tokio runtime?
//!
//! The stdlib's `AgentDispatch` callback signature is synchronous
//! `Fn(Request) -> Response`. We adapt by capturing the runtime's
//! `Handle` and using `tokio::runtime::Handle::block_on` to drive the
//! async `ask`. This avoids dragging async machinery into stdlib.

use crate::agent::AgentHandle;
use crate::runtime::Runtime;
use mty_ir::interp::value::Value;
use std::sync::Arc;
use std::time::Duration;

/// Re-export of the stdlib request/response shape so callers in the
/// runtime crate don't have to depend on stdlib directly. Mirrors
/// `mty_stdlib::http::{Request, Response}`. We expose a structurally
/// identical shape so a downstream wire-up (`install_agent_dispatcher`)
/// can be written against either crate.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

/// Build a synchronous dispatcher closure that posts every incoming
/// request to the supplied agent as `agent?msg_name(req_json)` and
/// translates the reply into a `Response`. The closure captures an
/// `Arc<Runtime>` so it stays alive across the agent's mailbox lifetime,
/// plus the runtime's owning tokio `Handle` so blocking dispatch always
/// runs against that runtime even when invoked from inside the stdlib's
/// separate HTTP server runtime.
pub fn make_dispatcher(
    rt: Arc<Runtime>,
    handle: AgentHandle,
    msg_name: impl Into<String>,
) -> impl Fn(Request) -> Response + Send + Sync + Clone + 'static {
    let msg = msg_name.into();
    // Snapshot the runtime's own tokio handle. The stdlib HTTP server
    // runs its own multi_thread runtime; we cannot use the local
    // `Handle::current()` from inside the dispatcher because that
    // would be the stdlib runtime, not ours.
    let rt_tokio = rt.scheduler.rt.clone();
    move |req: Request| -> Response {
        let body_str = String::from_utf8_lossy(&req.body).to_string();
        let req_json = format!(
            "{{\"body\":\"{}\",\"method\":\"{}\",\"path\":\"{}\"}}",
            escape_json(&body_str),
            escape_json(&req.method),
            escape_json(&req.path),
        );
        let args = vec![Value::Str(req_json)];

        let deadline = Some(Duration::from_secs(5));
        let rt_inner = rt.clone();
        let h2 = handle.clone();
        let msg2 = msg.clone();
        // The closure runs on a stdlib tokio thread; `block_on`
        // there would panic. Spawn the work onto OUR runtime via
        // its captured handle and wait for the receiver on a
        // disposable std mpsc — that doesn't require a tokio
        // context.
        let (tx, rx) = std::sync::mpsc::channel::<crate::error::RuntimeResult<Value>>();
        rt_tokio.spawn(async move {
            let r = rt_inner.ask(&h2, &msg2, args, deadline).await;
            let _ = tx.send(r);
        });
        let reply = rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| {
                Err(crate::error::RuntimeError::Trap {
                    code: "MT5099",
                    message: "http dispatcher reply channel timed out".into(),
                })
            });

        match reply {
            Ok(value) => response_from_value(value),
            Err(e) => Response {
                status: 500,
                body: format!("agent dispatch error: {}", e).into_bytes(),
                headers: vec![("content-type".into(), "text/plain".into())],
            },
        }
    }
}

/// Convert a handler reply [`Value`] into a [`Response`]. Recognised
/// shapes:
///
///   * `Value::Str(json_with_status)` → if the string parses as a JSON
///     object with `{status, body}`, those become the response. Otherwise
///     the raw string is the body with HTTP 200.
///   * `Value::Int(n)` → empty body, status `n`.
///   * Anything else → 200 with the value's debug-printed form.
pub fn response_from_value(v: Value) -> Response {
    match v {
        Value::Str(s) => {
            // Quick heuristic: only attempt JSON-object parse if it
            // starts with `{`. Avoids dragging serde into the runtime.
            let trimmed = s.trim();
            if trimmed.starts_with('{') && trimmed.contains("\"status\"") {
                if let Some((status, body)) = parse_status_body(trimmed) {
                    return Response {
                        status,
                        body: body.into_bytes(),
                        headers: vec![("content-type".into(), "application/json".into())],
                    };
                }
            }
            Response {
                status: 200,
                body: s.into_bytes(),
                headers: vec![("content-type".into(), "application/json".into())],
            }
        }
        Value::Int(n, _) => Response {
            status: n.max(0) as u16,
            body: vec![],
            headers: vec![],
        },
        other => Response {
            status: 200,
            body: format!("{:?}", other).into_bytes(),
            headers: vec![("content-type".into(), "text/plain".into())],
        },
    }
}

/// Hand-rolled mini JSON parser for `{"status": N, "body": "S"}` so
/// the runtime doesn't need to pull in serde or pry into stdlib.
fn parse_status_body(s: &str) -> Option<(u16, String)> {
    let status = extract_json_int(s, "status")? as u16;
    let body = extract_json_str(s, "body").unwrap_or_default();
    Some((status, body))
}

fn extract_json_int(s: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{}\"", key);
    let idx = s.find(&needle)?;
    let after = &s[idx + needle.len()..];
    let colon = after.find(':')?;
    let mut rest = after[colon + 1..].trim_start();
    let mut num = String::new();
    while let Some(c) = rest.chars().next() {
        if c.is_ascii_digit() || c == '-' {
            num.push(c);
            rest = &rest[1..];
        } else {
            break;
        }
    }
    num.parse().ok()
}

fn extract_json_str(s: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let idx = s.find(&needle)?;
    let after = &s[idx + needle.len()..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let close = rest.find('"')?;
    Some(rest[..close].to_string())
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_from_plain_string_is_200() {
        let r = response_from_value(Value::Str("hello world".into()));
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"hello world");
    }

    #[test]
    fn response_from_status_object_uses_status() {
        let r = response_from_value(Value::Str(
            "{\"status\": 404, \"body\": \"not found\"}".into(),
        ));
        assert_eq!(r.status, 404);
        assert_eq!(r.body, b"not found");
    }

    #[test]
    fn escape_json_handles_specials() {
        assert_eq!(escape_json("a\"b"), "a\\\"b");
        assert_eq!(escape_json("a\nb"), "a\\nb");
        assert_eq!(escape_json("a\\b"), "a\\\\b");
    }

    #[test]
    fn extract_json_int_and_str() {
        let s = "{\"a\":1,\"b\":\"hi\",\"c\":42}";
        assert_eq!(extract_json_int(s, "a"), Some(1));
        assert_eq!(extract_json_int(s, "c"), Some(42));
        assert_eq!(extract_json_str(s, "b").as_deref(), Some("hi"));
    }
}
