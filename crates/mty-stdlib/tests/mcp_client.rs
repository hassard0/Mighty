//! Integration tests for the MCP client (v0.26 Track B).
//!
//! Drive an in-process MCP server over a pair of `os_pipe`-style
//! byte streams and exercise the client's initialize / list / call
//! handlers. The transport surface is intentionally minimalist (two
//! `Read` / `Write` halves) so tests don't need a real subprocess.

use mty_stdlib::mcp::{
    clear_registry_for_tests, register_tool, CapabilitySet, McpClient, McpServer, ParamFieldSchema,
    RegisteredTool, ToolDescriptor, ToolParameterSchema,
};
use serde_json::json;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;

/// Global lock to serialise tests that mutate the process-wide tool
/// registry — same rationale as `mcp_server.rs`.
static REGISTRY_LOCK: Mutex<()> = Mutex::new(());

/// Build an in-memory bidirectional channel between two threads. The
/// first returned pair belongs to the "client side" (it WRITES
/// requests to `client_tx`, READS responses from `client_rx`); the
/// second pair belongs to the "server side" (it READS requests from
/// `server_rx`, WRITES responses to `server_tx`).
///
/// Implementation: each direction is an `os_pipe`-style pair built
/// from `std::sync::mpsc::channel` + a small adapter that wraps the
/// channel halves in `Read`/`Write` blocking IO. Avoids pulling
/// `os_pipe` in as a dep.
fn make_pipe_pair() -> (PipeWriter, PipeReader) {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let writer = PipeWriter {
        tx,
        buf: Vec::new(),
    };
    let reader = PipeReader {
        rx,
        buf: Vec::new(),
        eof: false,
    };
    (writer, reader)
}

struct PipeWriter {
    tx: std::sync::mpsc::Sender<Vec<u8>>,
    buf: Vec<u8>,
}

impl Write for PipeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(buf);
        // Flush eagerly on each write to keep the reader unblocked.
        // The MCP framing is newline-delimited, so on every newline
        // we ship the accumulated buffer.
        if let Some(idx) = self.buf.iter().rposition(|&b| b == b'\n') {
            let prefix: Vec<u8> = self.buf.drain(..=idx).collect();
            // Sender::send returns Err if the receiver has been
            // dropped — translate to a broken-pipe IO error so the
            // server's serve_io loop exits cleanly.
            self.tx
                .send(prefix)
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe closed"))?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if !self.buf.is_empty() {
            let payload = std::mem::take(&mut self.buf);
            self.tx
                .send(payload)
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe closed"))?;
        }
        Ok(())
    }
}

struct PipeReader {
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
    buf: Vec<u8>,
    eof: bool,
}

impl Read for PipeReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.buf.is_empty() {
            if self.eof {
                return Ok(0);
            }
            match self.rx.recv() {
                Ok(chunk) => self.buf.extend(chunk),
                Err(_) => {
                    self.eof = true;
                    return Ok(0);
                }
            }
        }
        let n = self.buf.len().min(out.len());
        out[..n].copy_from_slice(&self.buf[..n]);
        self.buf.drain(..n);
        Ok(n)
    }
}

/// Spawn an MCP server in a thread and return a client wired to it.
/// The server uses an unrestricted cap-set; tests that need narrower
/// caps build their own server.
fn spawn_server(srv: McpServer) -> McpClient<PipeReader, PipeWriter> {
    let (client_tx, server_rx) = make_pipe_pair();
    let (server_tx, client_rx) = make_pipe_pair();
    thread::spawn(move || {
        let _ = srv.serve_io(server_rx, server_tx);
    });
    McpClient::new(client_rx, client_tx)
}

fn make_echo_tool(name: &str) -> RegisteredTool {
    let mut props = HashMap::new();
    props.insert("text".to_string(), ParamFieldSchema::primitive("string"));
    RegisteredTool {
        descriptor: ToolDescriptor {
            name: name.to_string(),
            description: format!("Echo tool {name}"),
            input_schema: ToolParameterSchema {
                ty: "object".into(),
                properties: props,
                required: vec!["text".into()],
            },
            capability: None,
        },
        invoke: Arc::new(|args, _caps| {
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            Ok(json!(format!("echoed: {text}")))
        }),
    }
}

#[test]
fn client_connects_to_test_server() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    register_tool(make_echo_tool("echo"));

    let srv = McpServer::from_tool_registry().with_capabilities(CapabilitySet::unrestricted());
    let mut client = spawn_server(srv);

    let version = client.initialize().expect("initialize");
    assert_eq!(version, mty_stdlib::mcp::MCP_PROTOCOL_VERSION);

    let tools = client.list_tools().expect("list_tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");
    clear_registry_for_tests();
}

#[test]
fn client_call_tool_round_trip() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    register_tool(make_echo_tool("echo"));

    let srv = McpServer::from_tool_registry().with_capabilities(CapabilitySet::unrestricted());
    let mut client = spawn_server(srv);

    let _ = client.initialize().expect("initialize");
    let text = client
        .call_tool_text("echo", json!({ "text": "round-trip" }))
        .expect("call_tool_text");
    assert_eq!(text, "echoed: round-trip");
    clear_registry_for_tests();
}

#[test]
fn client_call_unknown_tool_returns_protocol_error() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();

    let srv = McpServer::from_tool_registry().with_capabilities(CapabilitySet::unrestricted());
    let mut client = spawn_server(srv);

    let _ = client.initialize().expect("initialize");
    let err = client.call_tool("nope", json!({})).unwrap_err();
    match err {
        mty_stdlib::mcp::McpError::Tool(mty_stdlib::mcp::ToolError::Protocol { detail }) => {
            assert!(detail.contains("-32601"), "detail: {detail}");
        }
        other => panic!("expected Protocol err, got {other:?}"),
    }
}

#[test]
fn client_list_tools_returns_descriptors_in_sorted_order() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    register_tool(make_echo_tool("zeta"));
    register_tool(make_echo_tool("alpha"));
    register_tool(make_echo_tool("mu"));

    let srv = McpServer::from_tool_registry().with_capabilities(CapabilitySet::unrestricted());
    let mut client = spawn_server(srv);
    let _ = client.initialize().expect("initialize");

    let tools = client.list_tools().expect("list_tools");
    let names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
    assert_eq!(names, vec!["alpha", "mu", "zeta"]);
    clear_registry_for_tests();
}

#[test]
fn client_call_tool_propagates_capability_denied_as_protocol_error() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    let mut props = HashMap::new();
    props.insert("path".to_string(), ParamFieldSchema::primitive("string"));
    register_tool(RegisteredTool {
        descriptor: ToolDescriptor {
            name: "read".into(),
            description: "Read".into(),
            input_schema: ToolParameterSchema {
                ty: "object".into(),
                properties: props,
                required: vec!["path".into()],
            },
            capability: Some("fs.read".into()),
        },
        invoke: Arc::new(|args, caps| {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            mty_stdlib::mcp::require_capability("read", "fs.read", path, caps)?;
            Ok(json!("ok"))
        }),
    });

    // Empty cap-set — server should refuse the call.
    let srv = McpServer::from_tool_registry().with_capabilities(CapabilitySet::empty());
    let mut client = spawn_server(srv);
    let _ = client.initialize().expect("initialize");
    let err = client
        .call_tool("read", json!({ "path": "/etc/passwd" }))
        .unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("capability") || msg.contains("-32001"),
        "{msg}"
    );
    clear_registry_for_tests();
}
