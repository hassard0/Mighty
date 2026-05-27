//! MCP transports — stdio (canonical) + HTTP (JSON-RPC body).
//!
//! The MCP spec defines two transports:
//!
//! - **stdio.** The server reads newline-delimited JSON-RPC from
//!   stdin and writes responses to stdout. Used when an MCP client
//!   spawns the server as a child process (the dominant deployment
//!   shape today).
//! - **HTTP.** The server accepts POSTs of a single JSON-RPC request
//!   body and replies with the JSON-RPC response. Used for
//!   long-running daemons.
//!
//! Both transports share the same JSON-RPC framing — only the byte
//! stream differs.

use super::{JsonRpcRequest, JsonRpcResponse, McpError};
use std::io::{BufRead, BufReader, Read, Write};

/// Trait for one side of an MCP conversation. Both stdio and HTTP
/// transports lower into this shape so the [`super::McpServer`] /
/// [`super::McpClient`] can be agnostic.
///
/// The trait is intentionally NOT `Send` — the stdio transport
/// instance built on top of `stdin().lock()` carries a
/// [`std::io::StdinLock`] whose guard is `!Send` on Windows. Code
/// that needs to hand the transport across threads should wrap it
/// in `Arc<Mutex<dyn Transport>>` or use the `IoMcpClient` shape
/// directly with `Send` readers/writers.
pub trait Transport {
    /// Read one JSON-RPC frame from the peer. Returns `Ok(None)` on
    /// EOF (clean disconnect).
    fn read_frame(&mut self) -> Result<Option<JsonRpcRequest>, McpError>;
    /// Write one JSON-RPC frame to the peer.
    fn write_frame(&mut self, frame: &JsonRpcResponse) -> Result<(), McpError>;
}

/// stdio transport — newline-delimited JSON-RPC over a pair of
/// [`Read`] / [`Write`] streams.
///
/// In production, instantiate as
/// `StdioTransport::new(std::io::stdin(), std::io::stdout())`. Tests
/// use in-memory pipes (e.g. `Vec<u8>` for output, `&[u8]` for
/// input) for hermetic round-trips.
pub struct StdioTransport<R: Read, W: Write> {
    reader: BufReader<R>,
    writer: W,
}

impl<R: Read, W: Write> StdioTransport<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader),
            writer,
        }
    }
}

impl<R: Read, W: Write> Transport for StdioTransport<R, W> {
    fn read_frame(&mut self) -> Result<Option<JsonRpcRequest>, McpError> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Skip blank lines; some MCP clients pad with empty
            // newlines between frames.
            return self.read_frame();
        }
        let req: JsonRpcRequest = serde_json::from_str(trimmed)?;
        Ok(Some(req))
    }

    fn write_frame(&mut self, frame: &JsonRpcResponse) -> Result<(), McpError> {
        let bytes = serde_json::to_vec(frame)?;
        self.writer.write_all(&bytes)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }
}

/// Convenience: render a JSON-RPC response into a newline-terminated
/// byte string. Used by the HTTP transport and tests.
pub fn encode_frame(frame: &JsonRpcResponse) -> Result<Vec<u8>, McpError> {
    let mut bytes = serde_json::to_vec(frame)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Convenience: parse a single JSON-RPC request from a byte slice.
/// Used by the HTTP transport (which has a single body, not a stream).
pub fn decode_frame(bytes: &[u8]) -> Result<JsonRpcRequest, McpError> {
    let req: JsonRpcRequest = serde_json::from_slice(bytes)?;
    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::super::{JsonRpcResponse, JSONRPC_VERSION};
    use super::*;
    use serde_json::json;

    #[test]
    fn stdio_round_trip_reads_one_frame_and_writes_one() {
        let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n".to_vec();
        let mut output: Vec<u8> = Vec::new();
        let mut t = StdioTransport::new(input.as_slice(), &mut output);
        let req = t.read_frame().unwrap().unwrap();
        assert_eq!(req.method, "tools/list");
        let resp = JsonRpcResponse::success(json!(1), json!({"tools": []}));
        t.write_frame(&resp).unwrap();
        // Drop t so the writer borrow releases.
        drop(t);
        let s = String::from_utf8(output).unwrap();
        assert!(s.starts_with("{"), "got: {s}");
        assert!(s.ends_with("\n"), "got: {s}");
        assert!(s.contains("\"tools\""), "got: {s}");
    }

    #[test]
    fn stdio_skips_blank_lines() {
        let input = b"\n\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n".to_vec();
        let output: Vec<u8> = Vec::new();
        let mut t = StdioTransport::new(input.as_slice(), output);
        let req = t.read_frame().unwrap().unwrap();
        assert_eq!(req.method, "ping");
    }

    #[test]
    fn stdio_returns_none_on_eof() {
        let input: &[u8] = b"";
        let output: Vec<u8> = Vec::new();
        let mut t = StdioTransport::new(input, output);
        assert!(t.read_frame().unwrap().is_none());
    }

    #[test]
    fn encode_decode_round_trip() {
        let resp = JsonRpcResponse::success(json!("abc"), json!({"ok": true}));
        let bytes = encode_frame(&resp).unwrap();
        assert!(bytes.ends_with(b"\n"));
        // Strip trailing newline and re-parse as request — won't
        // round-trip via decode_frame (which expects requests), but
        // we can verify the JSON shape.
        let s = std::str::from_utf8(&bytes).unwrap();
        let v: serde_json::Value = serde_json::from_str(s.trim()).unwrap();
        assert_eq!(
            v.get("jsonrpc").and_then(|j| j.as_str()),
            Some(JSONRPC_VERSION)
        );
    }
}
