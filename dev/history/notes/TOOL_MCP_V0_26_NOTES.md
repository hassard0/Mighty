# `@tool` macro + `std.mcp` — v0.26 Track B notes

Track B for v0.26 ships the killer feature that positions Mighty as
"the standard language for agents":

```mty
@tool("Read a file from disk", cap: fs.read)
fn read_file(path: String) -> Result[String, FsError] !{fs} {
  std.fs.read_to_string(path)
}
```

The `@tool` macro auto-generates the JSON schema for any LLM provider
AND auto-exposes the agent as an MCP server. The `cap:` annotation is
**enforced by the runtime** — the tool cannot read paths outside its
capability set, no matter what the LLM prompts.

## What ships

### `mty-macros::stdlib::tool` — the `@tool` attribute expander

A code-driven builtin attribute macro (sibling to `format!`'s
code-driven builtin call macro). Exposes:

- `ParsedFn` / `ParsedParam` — light-weight view of the user's fn the
  HIR preprocessor passes in.
- `expand_tool_attribute(attr_args, func) -> Result<ToolExpansion, _>`
  — the main entry point.
- `ToolExpansion { original_decl, synthesised_decls,
  descriptor_json }` — the macro's three companion fns + the
  unchanged user fn + the descriptor as JSON.
- `BUILTIN_ATTRIBUTE_NAMES` / `is_builtin_attribute` /
  `expand_builtin_attribute` — the attribute-macro registry sibling
  to the existing call-macro registry.

The macro is serde-free (the macros crate has no JSON dep). JSON
text for the descriptor is built by hand via small string
concatenation helpers — the surface is tiny and deterministic.

Diagnostic codes added: **MT6011..MT6016** (NotAFn,
MissingDescription, DescriptionNotALiteral, MalformedCap,
GenericNotSupported, ParamMissingType).

### `mty-stdlib::mcp` — runtime registry + server/client + sandbox

Five files under `crates/mty-stdlib/src/mcp/`:

| File | Lines | Role |
|---|---|---|
| `mod.rs` | ~400 | JSON-RPC types, registry, error shapes, register-from-JSON, require-capability. |
| `sandbox.rs` | ~310 | `CapabilitySet`, `CapabilityGrant` (Fs/Net/Clock/Model/Custom), default-cap-set installation. |
| `server.rs` | ~250 | `McpServer` dispatch (initialize/ping/tools-list/tools-call). |
| `client.rs` | ~210 | `McpClient` round-trip + `connect_stdio` convenience. |
| `transport.rs` | ~130 | Stdio JSON-RPC framing. |

Wire shape mirrors the upstream MCP spec at
<https://modelcontextprotocol.io/specification/2024-11-05>.

### Capability families

Five built-in grant shapes:

```rust
CapabilityGrant::Fs { mode: FsMode::Read | FsMode::ReadWrite, roots: Vec<PathBuf> }
CapabilityGrant::Net { hosts: Vec<String> }       // suffix-matched
CapabilityGrant::Clock
CapabilityGrant::Model { providers: Vec<String> }
CapabilityGrant::Custom { family, resources }
```

Empty `roots` / `hosts` / `providers` = unrestricted within the
family. Suffix matching for `Net` grants subdomains automatically
(`example.com` grants `api.example.com`). The `Custom` grant routes
by string tag for app-defined cap families.

The check function is `caps.check(required, resource) -> Result<(), String>`;
the convenience wrapper `mty_stdlib::mcp::require_capability(tool,
required, resource, caps) -> Result<(), ToolError>` is what tool
invoke closures call — it formats the denial into the standard
`ToolError::CapabilityDenied` shape.

## Architecture

The MCP server's `handle_tools_call` flow:

1. Extract `name` + `arguments` from the JSON-RPC params object.
2. Snapshot the active cap-set (server-bound override or process-wide
   default).
3. Call `invoke_tool(name, args, &caps)` — looks up the registered
   tool, hands the closure both halves.
4. The closure calls `require_capability` **first** — this is the
   load-bearing guarantee. Even if a buggy tool body forgets the cap
   check, the `@tool` macro inserts it automatically.
5. On success, wrap the JSON result in MCP's `{content: [{type:
   "text", text: ...}]}` envelope.
6. On failure, emit a JSON-RPC error object with the spec-mandated
   code (-32000..-32099 server range, -32601 method-not-found,
   -32602 invalid-params, -32001 capability-denied).

## Diagnostic codes added

| Code | Meaning |
|---|---|
| MT6011 | `@tool` decorates a non-fn item. |
| MT6012 | `@tool()` requires a description string. |
| MT6013 | Description must be a string literal. |
| MT6014 | `cap:` arg must be a dotted path. |
| MT6015 | `@tool` on a generic fn (concrete types only). |
| MT6016 | `@tool` fn param missing type annotation. |

Surfaced by `mty_macros::ToolMacroError`. The HIR preprocessor wires
them in v0.27 when attribute-macro plumbing lands; until then
callers (Track A LLM crate, Track E demos) use the typed enum
directly.

## Test coverage

| Suite | Tests | Notes |
|---|---|---|
| `mty-macros::stdlib::tool` lib tests | 15 | parse/render helpers, expansion happy + error paths. |
| `mty-macros::tests::tool_macro` | 13 | end-to-end expansion contract. |
| `mty-stdlib::tests::mcp_server` | 7 | tools/list, tools/call, stdio round-trip, capability-denied, sorted-descriptor. |
| `mty-stdlib::tests::mcp_client` | 5 | initialize, list, call, unknown-tool, cap-denied propagation. |
| `mty-stdlib::tests::tool_cap_enforcement` | 8 | matching/missing/narrowing/outside-scope/read-vs-rw/net-suffix/unknown-tool. |

Total: **48 new tests**, all green.

## What Track E can consume today

Track E demos can build a working agent end-to-end with just the
Rust-side API (no waiting for HIR-preprocessor attribute-macro
plumbing):

```rust
use mty_stdlib::mcp::*;
use serde_json::json;
use std::sync::Arc;

// 1. Register tools imperatively.
register_tool(RegisteredTool {
    descriptor: ToolDescriptor {
        name: "read_file".into(),
        description: "Read a file".into(),
        input_schema: /* ... */,
        capability: Some("fs.read".into()),
    },
    invoke: Arc::new(|args, caps| {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
        require_capability("read_file", "fs.read", path, caps)?;
        // call real impl
        Ok(json!("file contents..."))
    }),
});

// 2. Install a narrow cap-set.
install_default_capability_set(CapabilitySet::from_grants([
    CapabilityGrant::Fs { mode: FsMode::Read, roots: vec!["/data".into()] },
]));

// 3. Serve.
let server = McpServer::from_tool_registry();
server.serve_stdio_blocking()?;
```

Or call OTHER MCP servers via the client:

```rust
let mut client = connect_stdio(Command::new("mcp-server-foo"))?;
let _version = client.initialize()?;
let tools = client.list_tools()?;
let answer = client.call_tool_text("read_file", json!({"path": "/data/x"}))?;
```

## v0.26 boundaries (deferred)

- **Async transport.** `serve_io` is blocking; async lands in v0.27.
- **WebSocket / SSE.** Stdio + HTTP only at v0.26.
- **Single in-flight request per client.** Concurrent calls require
  v0.27 multiplex.
- **`@tool` invoke-body typed marshalling.** The macro's synth
  `__tool_invoke_<NAME>` is a placeholder; real arg deserialisation
  lands when `std.json` gains a typed ADT (v0.27). The Rust-side
  runtime wrapper already implements typed marshalling for native
  callers — Track E demos use that path.
- **Multi-cap declarations.** `cap: fs.read, net.get` not yet
  supported; declare one and let the body's effect set carry the
  rest.
- **HIR-preprocessor attribute plumbing.** The `@<attr>` surface
  syntax isn't in the parser yet. The macro exposes a stable Rust API
  for downstream callers (Track A LLM crate, Track E demos); when the
  HIR layer lifts attribute macros it just routes through
  `expand_builtin_attribute`.

## Cross-track integration

| Track | Touchpoint |
|---|---|
| A (`std.llm`) | LLM clients accept `ToolDescriptor` directly — the descriptor's JSON-schema input matches Anthropic / OpenAI / Gemini tool shapes. |
| C (`std.memory`) | Memory mutations can be exposed as tools (`@tool("Search memory", cap: model.call)`) — the cap-set narrows what the LLM can interrogate. |
| D (codegen) | When attribute-macro plumbing lands in the HIR, the SIR layer emits a call to `__tool_register_<NAME>` from module init. v0.26 wiring leaves that hook for v0.27. |
| E (demos) | Can build a full agent today via the imperative Rust API. The `@tool` source-level form ships as documentation; demos that want it inline can use the Rust-side `register_tool` directly. |

## Files added / modified

NEW:

- `crates/mty-macros/src/stdlib/tool.rs`
- `crates/mty-macros/tests/tool_macro.rs`
- `crates/mty-stdlib/src/mcp/mod.rs`
- `crates/mty-stdlib/src/mcp/sandbox.rs`
- `crates/mty-stdlib/src/mcp/server.rs`
- `crates/mty-stdlib/src/mcp/client.rs`
- `crates/mty-stdlib/src/mcp/transport.rs`
- `crates/mty-stdlib/tests/mcp_server.rs`
- `crates/mty-stdlib/tests/mcp_client.rs`
- `crates/mty-stdlib/tests/tool_cap_enforcement.rs`
- `docs/reference/stdlib/mcp.md`
- `docs/reference/macros/tool.md`
- `dev/history/notes/TOOL_MCP_V0_26_NOTES.md` (this file)

EXTENDED:

- `crates/mty-macros/src/stdlib.rs` (register `tool` builtin attribute)
- `crates/mty-macros/src/lib.rs` (re-export macro + descriptor surface)
- `crates/mty-stdlib/src/lib.rs` (`pub mod mcp;` + docline)
- `crates/mty-types/src/prelude.rs` (register `std.mcp` opaque module)
