# Mighty intro

Mighty is an agent-first language. Every program lowers to an agent
graph: typed protocols carry messages between actors, the runtime
schedules them, and the borrow checker enforces single-writer
semantics across the whole graph.

## Capability typing

Capability typing is the cornerstone of Mighty's safety model. Every
value carries a typed effect row: `net`, `fs`, `model`, `dom`, `spawn`.
The compiler refuses unsafe combinations at the call site, so a value
that came from `std.fs.read` can never sneak into an `std.net.post`
without an explicit untainting step.

## Multi-modal

The `std.llm` provider abstraction in v0.33 supports image input
across all four providers (Anthropic, OpenAI, Gemini, Bedrock). The
`std.rag.Rag` pipeline lifts that into a one-liner `ask_with_image`
call that grounds the answer in both retrieved text context and the
visual input.
