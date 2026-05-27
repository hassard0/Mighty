//! MCP client — calls OTHER MCP servers from inside a Mighty agent.
//!
//! Mirrors the [`super::McpServer`] handler set on the consuming
//! side: `initialize` → `tools/list` → `tools/call`. The transport
//! is again either stdio (over a spawned child process) or a paired
//! [`Read`] + [`Write`] for tests.
//!
//! The client owns its own read/write halves (rather than going
//! through the symmetric [`super::Transport`] trait that the server
//! uses) because the client SENDS requests and RECEIVES responses —
//! the reverse of what `Transport::read_frame`/`write_frame`
//! provides. A bidirectional transport trait that handles both shapes
//! is a v0.27 follow-up.

use super::{
    JsonRpcRequest, JsonRpcResponse, McpError, ToolDescriptor, ToolError, MCP_PROTOCOL_VERSION,
};
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicI64, Ordering};

/// MCP client for a single server connection. Sends requests
/// sequentially (one in-flight at a time). The id counter is
/// monotonic across the connection lifetime so each request has a
/// unique id even after reconnects.
///
/// Use [`McpClient::new`] with any `Read` + `Write` pair (for tests:
/// the read+write halves of an in-memory pipe; for production: the
/// `ChildStdout`/`ChildStdin` of a spawned MCP server process — see
/// [`connect_stdio`]).
pub struct McpClient<R: Read, W: Write> {
    reader: BufReader<R>,
    writer: W,
    next_id: AtomicI64,
}

impl<R: Read, W: Write> McpClient<R, W> {
    /// Build a client around a paired reader + writer. Sends
    /// requests via `writer`, reads responses via `reader`.
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader),
            writer,
            next_id: AtomicI64::new(1),
        }
    }

    /// Send the MCP `initialize` handshake. Returns the server's
    /// reported protocol version; fails if the server replies with
    /// an error or an unexpected shape.
    pub fn initialize(&mut self) -> Result<String, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let req = JsonRpcRequest::new(
            id,
            "initialize",
            serde_json::json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "clientInfo": {
                    "name": "mty-mcp-client",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        );
        let resp = self.round_trip(&req)?;
        let result = result_or_err(resp)?;
        result
            .get("protocolVersion")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                McpError::Transport("initialize response missing protocolVersion".into())
            })
    }

    /// `tools/list` — fetch every tool the server exposes.
    pub fn list_tools(&mut self) -> Result<Vec<ToolDescriptor>, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let req = JsonRpcRequest::new(id, "tools/list", serde_json::json!({}));
        let resp = self.round_trip(&req)?;
        let result = result_or_err(resp)?;
        let tools = result
            .get("tools")
            .cloned()
            .ok_or_else(|| McpError::Transport("tools/list response missing tools".into()))?;
        let descriptors: Vec<ToolDescriptor> = serde_json::from_value(tools)?;
        Ok(descriptors)
    }

    /// `tools/call` — invoke a named tool on the server with the
    /// given JSON arguments. Returns the raw JSON result block — most
    /// callers will use [`call_tool_text`](Self::call_tool_text) for
    /// the common case where the server returns a single text
    /// content block.
    pub fn call_tool(
        &mut self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let req = JsonRpcRequest::new(
            id,
            "tools/call",
            serde_json::json!({
                "name": name,
                "arguments": args,
            }),
        );
        let resp = self.round_trip(&req)?;
        result_or_err(resp)
    }

    /// Convenience: invoke a tool and extract the first text content
    /// block. Fails with `Transport(...)` if the result has no
    /// `content[0].text` field.
    pub fn call_tool_text(
        &mut self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<String, McpError> {
        let result = self.call_tool(name, args)?;
        result
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("text"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                McpError::Transport(format!(
                    "tools/call response has no text content block: {result}"
                ))
            })
    }

    /// Send a notification (no response expected). Used for MCP
    /// progress / log events.
    pub fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<(), McpError> {
        let req = JsonRpcRequest {
            jsonrpc: super::JSONRPC_VERSION.to_string(),
            id: None,
            method: method.to_string(),
            params: Some(params),
        };
        let bytes = serde_json::to_vec(&req)?;
        self.writer.write_all(&bytes)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }

    fn round_trip(&mut self, req: &JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let bytes = serde_json::to_vec(req)?;
        self.writer.write_all(&bytes)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.reader.read_line(&mut line)?;
            if n == 0 {
                return Err(McpError::Transport(
                    "server closed connection before responding".into(),
                ));
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue; // skip blank framing
            }
            let resp: JsonRpcResponse = serde_json::from_str(trimmed)?;
            return Ok(resp);
        }
    }
}

/// Spawn `cmd` as a child process and build a client around its
/// stdio pipes. Available only when `cmd` is fully configured by the
/// caller (env, working-dir, args set up).
///
/// This is a convenience for the common
/// `Command::new("mcp-server-foo")` shape; advanced callers can
/// build [`McpClient`] directly from any reader/writer pair.
///
/// The returned client takes ownership of the child's stdin/stdout
/// pipes; the child handle is detached (callers that need to
/// supervise the child process should build the client manually).
pub fn connect_stdio(
    mut cmd: std::process::Command,
) -> Result<McpClient<std::process::ChildStdout, std::process::ChildStdin>, McpError> {
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| McpError::Transport("child stdin unavailable".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| McpError::Transport("child stdout unavailable".into()))?;
    // Intentionally drop the child handle — the pipes keep the
    // process alive for the client's lifetime. Production callers
    // that want to .wait() should build McpClient directly.
    drop(child);
    Ok(McpClient::new(stdout, stdin))
}

fn result_or_err(resp: JsonRpcResponse) -> Result<serde_json::Value, McpError> {
    if let Some(err) = resp.error {
        return Err(McpError::Tool(ToolError::Protocol {
            detail: format!("rpc error {}: {}", err.code, err.message),
        }));
    }
    resp.result
        .ok_or_else(|| McpError::Transport("response has neither result nor error".into()))
}

#[cfg(test)]
mod tests {
    use super::super::JsonRpcError;
    use super::*;

    #[test]
    fn result_or_err_unwraps_success() {
        let resp = JsonRpcResponse::success(serde_json::json!(1), serde_json::json!({"ok": true}));
        let r = result_or_err(resp).unwrap();
        assert_eq!(r.get("ok").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn result_or_err_propagates_error() {
        let resp = JsonRpcResponse::failure(
            serde_json::json!(1),
            JsonRpcError {
                code: -32001,
                message: "denied".into(),
                data: None,
            },
        );
        let err = result_or_err(resp).unwrap_err();
        match err {
            McpError::Tool(ToolError::Protocol { detail }) => {
                assert!(detail.contains("-32001"), "{detail}");
            }
            other => panic!("expected Protocol err, got {other:?}"),
        }
    }
}
