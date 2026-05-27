# `std.mcp`

Model Context Protocol server + client + `@tool` registry + capability-enforced sandboxing. Shipped in **v0.26 Track B** alongside Track A's `std.llm` and Track C's `std.memory` — the three modules together cover the "agent talks to a model, calls tools, and remembers what it did" loop.

`std.mcp` is what makes Mighty "the standard language for agents":

```mty
@tool("Read a file from disk", cap: fs.read)
fn read_file(path: String) -> Result[String, FsError] !{fs} {
  std.fs.read_to_string(path)
}

fn main() {
  let server = McpServer::from_tool_registry()
  server.serve_stdio().await.unwrap()
}
```

The `@tool` decorator auto-generates the descriptor's JSON schema (for any LLM provider) AND auto-exposes the fn over the canonical MCP wire protocol. The `cap: fs.read` annotation is enforced by the runtime — the tool cannot read paths outside its capability set, no matter what the LLM prompts.

## Three layers

### 1. Tool registry

Process-wide map of tool-name → `RegisteredTool`. Populated either by the `@tool` macro at module-init time (the typical path), or imperatively via `mty_stdlib::mcp::register_tool` for downstream code that builds tools dynamically.

Public API:

| Function | Purpose |
|---|---|
| `register_tool(RegisteredTool)` | Add or replace a tool. |
| `registered_tool_names() -> Vec<String>` | Sorted snapshot. |
| `registered_descriptors() -> Vec<ToolDescriptor>` | Sorted descriptors. |
| `invoke_tool(name, args, caps) -> Result<Value, ToolError>` | Direct call. |
| `register_tool_from_json(json) -> Result<(), McpError>` | Used by the `@tool` synth source. |

### 2. Capability-enforced sandbox

Every tool declares the caps it needs:

```mty
@tool("Read", cap: fs.read)       // fs read-only
@tool("Write", cap: fs.write)     // fs read-write
@tool("Fetch", cap: net.get)      // net (host-narrowable)
@tool("Now", cap: clock.now)      // wall-clock
@tool("Ask", cap: model.call)     // LLM provider (provider-narrowable)
```

At call time, the runtime consults the active `CapabilitySet` *before* invoking the tool body. If the grant is missing OR the requested resource is outside the granted scope, the call short-circuits with `ToolError::CapabilityDenied`. The LLM gets a tool-result describing the denial; the host never touches the resource.

Grant shapes:

```rust
CapabilityGrant::Fs { mode: FsMode::Read | FsMode::ReadWrite, roots: Vec<PathBuf> }
CapabilityGrant::Net { hosts: Vec<String> }       // suffix-matched
CapabilityGrant::Clock                            // boolean
CapabilityGrant::Model { providers: Vec<String> } // anthropic/openai/...
CapabilityGrant::Custom { family, resources }     // app-defined
```

Empty `roots` / `hosts` / `providers` = unrestricted within the family. Use `CapabilitySet::unrestricted()` for tests; production LLM-driven flows should ALWAYS narrow.

```rust
let caps = CapabilitySet::from_grants([
    CapabilityGrant::Fs { mode: FsMode::Read, roots: vec!["/data".into()] },
    CapabilityGrant::Net { hosts: vec!["api.example.com".into()] },
]);
let server = McpServer::from_tool_registry().with_capabilities(caps);
```

### 3. MCP transport

`McpServer` reads JSON-RPC 2.0 requests from a transport and routes them to the registry:

| Method | Behaviour |
|---|---|
| `initialize` | Returns `{protocolVersion, serverInfo, capabilities}`. |
| `ping` | Liveness probe; returns `{}`. |
| `tools/list` | Returns every registered descriptor (sorted by name). |
| `tools/call` | Looks up the named tool, runs the cap check, invokes the body. |

`McpClient` speaks the same protocol to other servers:

```rust
let mut client = mty_stdlib::mcp::connect_stdio(
    Command::new("mcp-server-foo")
)?;
let _version = client.initialize()?;
let tools = client.list_tools()?;
let answer = client.call_tool_text("read_file", json!({"path": "/data/x"}))?;
```

Both stdio (newline-delimited JSON) and HTTP (single JSON body per request) are supported. The protocol version advertised is `2024-11-05`.

## `@tool` macro

The full `@tool(...)` grammar:

```mty
@tool(<description>, cap: <cap-path>)
fn <name>(<params>) -> <ret-ty> [!{<effects>}] {
  <body>
}
```

| Field | Required? | Notes |
|---|---|---|
| description | yes | String literal, first positional arg. May also be `description: "..."`. |
| `cap:` | no | Dotted cap path (`fs.read`, `net.get`, `model.call`, `clock.now`, …). |

The macro expands to three companion fns + the user's original fn (returned verbatim):

- `__tool_descriptor_<NAME>() -> String` — returns the descriptor as JSON text.
- `__tool_invoke_<NAME>(args: String) -> String` — cap-checks, runs the body, returns the JSON result.
- `__tool_register_<NAME>()` — called by module init to populate the runtime registry.

### Parameter-type mapping

| Mighty type | JSON schema type |
|---|---|
| `String` / `Str` | `"string"` |
| `Bool` | `"boolean"` |
| `I8`..`I128`, `U8`..`U128`, `ISize`, `USize` | `"integer"` |
| `F32`, `F64` | `"number"` |
| `Vec[T]` | `"array"` with `items` from T |
| `Option[T]` | inner T's schema, field omitted from `required` |
| any other | `"object"` (free-form) |

### Diagnostics

| Code | Trigger |
|---|---|
| MT6011 | `@tool` decorates a non-fn item. |
| MT6012 | `@tool()` called with no arguments. |
| MT6013 | First arg is not a string literal. |
| MT6014 | `cap:` argument is not a dotted path. |
| MT6015 | Fn has generic params (concrete types only at v0.26). |
| MT6016 | Fn parameter has no type annotation. |

## v0.26 boundaries

- **No async transport.** The server's `serve_io` is blocking. Async support lands when the runtime grows a hosted `tokio` integration (v0.27).
- **Stdio + HTTP only.** WebSocket and sse transports are deferred.
- **Single in-flight request per client.** `round_trip` serializes sends and receives — fine for the typical "agent calls one tool at a time" shape; concurrent calls require a v0.27 multiplex.
- **Macro stub invoke.** The `@tool` macro's `__tool_invoke_<NAME>` body is a placeholder that documents the wired path; real arg-deserialisation lands when `std.json` gains a typed ADT (v0.27). The Rust-side runtime wrapper in `mty_stdlib::mcp::register_tool` already implements typed marshalling for native callers — Track E demos use that path.

## Implementation map

| File | What |
|---|---|
| `crates/mty-stdlib/src/mcp/mod.rs` | Wire types, registry, `register_tool_from_json`, `require_capability`. |
| `crates/mty-stdlib/src/mcp/sandbox.rs` | `CapabilitySet`, `CapabilityGrant`, default-cap-set installation. |
| `crates/mty-stdlib/src/mcp/server.rs` | `McpServer` request dispatch. |
| `crates/mty-stdlib/src/mcp/client.rs` | `McpClient`, `connect_stdio`. |
| `crates/mty-stdlib/src/mcp/transport.rs` | Stdio JSON-RPC framing. |
| `crates/mty-macros/src/stdlib/tool.rs` | `@tool` attribute macro expander. |

Tests:

- `crates/mty-macros/tests/tool_macro.rs` (13)
- `crates/mty-stdlib/tests/mcp_server.rs` (7)
- `crates/mty-stdlib/tests/mcp_client.rs` (5)
- `crates/mty-stdlib/tests/tool_cap_enforcement.rs` (8)
