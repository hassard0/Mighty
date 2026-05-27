# `@tool` attribute macro

The `@tool` decorator marks a Mighty fn as an LLM-callable tool. Shipped in **v0.26 Track B** alongside `std.mcp` (the runtime registry, MCP server/client, and capability-enforced sandbox).

```mty
@tool("Read a file from disk", cap: fs.read)
fn read_file(path: String) -> Result[String, FsError] !{fs} {
  std.fs.read_to_string(path)
}
```

## What it does

For each `@tool`-annotated fn, the macro generates three companion items (the user's original fn is returned unchanged):

1. `__tool_descriptor_<NAME>() -> String` — returns the descriptor as JSON text. Used when surfacing the tool to an LLM provider via `std.llm`.
2. `__tool_invoke_<NAME>(args: String) -> String` — deserialises the JSON args into typed params, runs the cap check (failing with `ToolError::CapabilityDenied` when needed), invokes the original fn, and serialises the return value.
3. `__tool_register_<NAME>()` — called at module init to populate the process-wide tool registry. After registration the tool is automatically exposed by any `McpServer::from_tool_registry()`.

## Surface grammar

```
@tool(<description>, [cap: <cap-path>])
```

| Field | Required? | Form |
|---|---|---|
| description | yes | String literal as the first positional arg. May also be `description: "..."`. |
| `cap:` | no | Dotted path (`fs.read`, `net.get`, `model.call`, `clock.now`, `secrets.read`, …). Bare family names are accepted too. |

The macro auto-derives the input schema from the fn's parameter types — see the type-mapping table in [`stdlib/mcp.md`](../stdlib/mcp.md).

## Examples

### Bare description, no cap

```mty
@tool("Add two numbers")
fn add(a: I32, b: I32) -> I32 {
  a + b
}
```

Tools without a `cap:` declaration default to the empty cap-set check — they pass through unconditionally. Use this only for pure functions that touch no host resources.

### Filesystem tool with narrow read cap

```mty
@tool("Read a file", cap: fs.read)
fn read_file(path: String) -> Result[String, FsError] !{fs} {
  std.fs.read_to_string(path)
}
```

At runtime, the active `CapabilitySet` is consulted before the body runs. If the active grant is `CapabilityGrant::Fs { mode: Read, roots: ["/data"] }` and the LLM prompts `read_file("/etc/passwd")`, the call short-circuits with `ToolError::CapabilityDenied` and the body never executes.

### Optional + array parameters

```mty
@tool("Search the web", cap: net.get)
fn search_web(query: String, tags: Vec[String], max_results: Option[I32]) -> Vec[String] {
  ...
}
```

`query` and `tags` are required; `max_results` is optional (its `Option[I32]` wrapper drops it from the JSON schema's `required` list).

## Diagnostics

| Code | Trigger |
|---|---|
| MT6011 | `@tool` decorates a non-fn item (struct, type alias, …). |
| MT6012 | `@tool()` called with no arguments. |
| MT6013 | First arg is not a string literal. |
| MT6014 | `cap:` argument is not a dotted path. |
| MT6015 | Fn has generic params (concrete types only at v0.26). |
| MT6016 | Fn parameter has no type annotation. |

## v0.26 boundaries

- **Concrete types only.** Generic fns (`fn read[T](x: T)`) are rejected because the JSON schema needs concrete types. Generic tools land in v0.27.
- **Macro stub invoke body.** The macro's `__tool_invoke_<NAME>` body is a placeholder that documents the wired path; full typed-arg marshalling lands when `std.json` grows a typed ADT (v0.27). The Rust-side runtime wrapper in `mty_stdlib::mcp::register_tool` already implements typed marshalling for native callers — Track E demos use that path.
- **Single-cap declarations.** Multiple caps per tool (`cap: fs.read, net.get`) are not yet supported; declare one and let the body's effect set carry the rest.

## Implementation

| File | What |
|---|---|
| `crates/mty-macros/src/stdlib/tool.rs` | Expander + `ToolMacroError`. |
| `crates/mty-macros/src/stdlib.rs` | `BUILTIN_ATTRIBUTE_NAMES`, `expand_builtin_attribute`. |
| `crates/mty-macros/tests/tool_macro.rs` | Integration tests. |

See [`stdlib/mcp.md`](../stdlib/mcp.md) for the runtime side (registry, server, client, sandbox).
