# Chapter 16 — Tools and LLM providers

Examples:
[`27_tool_attr.mty`](https://github.com/hassard0/Mighty/blob/main/examples/27_tool_attr.mty),
[`28_agent_with_llm_field.mty`](https://github.com/hassard0/Mighty/blob/main/examples/28_agent_with_llm_field.mty),
[`29_streaming.mty`](https://github.com/hassard0/Mighty/blob/main/examples/29_streaming.mty),
[`30_stream_consume.mty`](https://github.com/hassard0/Mighty/blob/main/examples/30_stream_consume.mty).

This is where Mighty's **LLM-agent** surface starts. Five pieces
compose into a real agent:

1. A `@tool(description, cap: ...)` decorator on a plain fn turns
   it into an LLM-visible tool with a JSON schema and a runtime-
   enforced capability set.
2. An LLM provider (`AnthropicClient`, `OpenAiClient`,
   `GeminiClient`, `BedrockClient`) carrying typed
   `complete()` / `messages_stream()` calls.
3. `Tainted[T]` on every provider response — the compiler tracks
   prompt-injection paths to sinks.
4. An agent that owns the panel, the budget, and the per-turn
   loop.
5. `std.swarm` to fan one prompt across multiple providers under
   one budget.

## The `@tool` decorator

```mty
@tool("Read a UTF-8 text file under ./data/.", cap: fs.read("./data/**"))
fn read_doc(path: Str) -> Str!IoErr {
  std.fs.read_to_string(path)?
}
```

What the macro generates:

- `__tool_descriptor_read_doc()` returns a `ToolDescriptor` with
  the JSON schema derived from the fn signature.
- `__tool_invoke_read_doc(args: Json) -> Result[Json, ToolErr]`
  parses `args`, calls `read_doc`, serialises the return.
- `__tool_register_read_doc(registry: &mut ToolRegistry)` plugs
  the pair into a registry for `Member::ask` to discover.

The `cap:` clause is enforced by the runtime. Any `std.fs.read`
call inside the tool body that tries to read outside
`./data/**` returns `Result::Err(forbidden: ...)`.

See [reference/macros/tool.md](../reference/macros/tool.md).

## A first LLM call

```mty
use std.llm.AnthropicClient

fn ask_once(client: AnthropicClient, q: Str) -> Tainted[Str] {
  client.complete(
    model = "claude-opus-4-7",
    system = "You are a helpful assistant.",
    prompt = q,
    max_tokens = 1024,
  )
}
```

Notice the return type is `Tainted[Str]`, not `Str`. The compiler
forces you to sanitise (or `Untaint::after(value, "audit
justification")`) before feeding the reply into a sink.

## Streaming

For long-running completions, `messages_stream()` returns a
`MessageStream` that yields `Delta` values:

```mty
let mut stream = client.messages_stream(model, system, prompt);
while let Some(delta) = stream.next() {
  match delta {
    Delta.Text(s)   => print(s),
    Delta.ToolUse(t) => handle_tool(t),
    Delta.End(stop) => break,
  }
}
```

The `while let Some(d) = stream.next()` form is v0.29 Track D.
See `examples/29_streaming.mty` and `examples/30_stream_consume.mty`.

## Try it

```bash
mty check examples/27_tool_attr.mty
mty check examples/28_agent_with_llm_field.mty
mty check examples/29_streaming.mty
mty check examples/30_stream_consume.mty
```

All four print `ok: <path>`. To actually fire a provider call,
see [Demo 07](https://github.com/hassard0/Mighty/blob/main/demos/07_research_agent/README.md).
