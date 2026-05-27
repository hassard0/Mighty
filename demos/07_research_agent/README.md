# demo 07 — research agent

The forcing-function demo for Mighty v0.26's three LLM-agent tracks.
A single ~200 LOC `.mty` file that spawns a `Researcher` agent which:

1. Indexes a small text corpus into a local vector store
   ([Track C — `std.memory::VectorStore`](../../crates/mty-stdlib/src/memory/vector.rs)).
2. Receives an `Ask(question)` message over its typed protocol mailbox.
3. Pulls the top-K most relevant docs from the vector store + stages
   them on a per-turn `Working` scratchpad
   ([Track C — `std.memory::Working`](../../crates/mty-stdlib/src/memory/working.rs)).
4. Builds a prompt + calls
   [`AnthropicClient.complete`](../../crates/mty-stdlib/src/llm/anthropic.rs)
   via [Track A — `std.llm`](../../crates/mty-stdlib/src/llm).
5. Records the question + answer into the
   [`Episodic`](../../crates/mty-stdlib/src/memory/episodic.rs) ring
   buffer for next turn.
6. Returns the answer body up the reply channel.

The three "tools" (`read_doc` / `save_answer` / `search_corpus`) are
documented as plain Mighty fns. The `@tool("desc", cap: fs.read)`
attribute syntax is shipped at the macro layer
([Track B — `@tool` macro](../../crates/mty-macros/src/stdlib/tool.rs))
but its parser hook lands in v0.27 — until then the fns stand in as
the source-spec the v0.27 attribute-macro pass will lift into
auto-generated `__tool_*` companions. See
[`DEMO07_RESEARCH_AGENT_V0_26_NOTES.md`](../../dev/history/notes/DEMO07_RESEARCH_AGENT_V0_26_NOTES.md)
for the full follow-up list.

## Layout

```
demos/07_research_agent/
├── README.md                            # this file
├── mighty.toml                          # standard host-profile package
├── smoke.sh                             # check + fmt + opt-in mock-LLM end-to-end
├── src/main.mty                         # the Researcher agent (~200 LOC)
├── tools/
│   ├── sample_corpus/                   # 5 short .txt files the agent indexes
│   │   ├── agent_features.txt
│   │   ├── mighty_overview.txt
│   │   ├── release_history.txt
│   │   ├── roadmap_post_v1.txt
│   │   └── stdlib_quick_ref.txt
│   └── mock_llm/server.py               # 60-line stub for offline smoke
└── answers/                             # save_answer() target (created on demand)
```

## Run

### Sanity check (no LLM call)

```bash
./target/debug/mty check demos/07_research_agent/src/main.mty
./target/debug/mty fmt --check demos/07_research_agent/src/main.mty
bash demos/07_research_agent/smoke.sh
```

### With a real Anthropic key

```bash
cargo build -p mty-cli
export ANTHROPIC_API_KEY=sk-ant-...
./target/debug/mty run demos/07_research_agent/src/main.mty
```

The demo's `main()` carries the canonical seed question
(`"What does std.memory provide in Mighty v0.26?"`) as a hard-coded
literal — `mty run <path>` does not yet accept `-- <argv>`
positional forwarding into `std.env.args()`. That is v0.27
follow-up #3 in the
[notes file](../../dev/history/notes/DEMO07_RESEARCH_AGENT_V0_26_NOTES.md).

The first line of output is the indexing log:

```
research_agent: indexed sample corpus into ./tools/vector.json
```

After Claude returns, the second line is the answer. `Episodic`
records the question + answer for the next turn; re-running with a
follow-up question (the v0.26 demo only does one turn — multi-turn
is a v0.27 follow-up; see notes §C) will see the previous turn in
its `recent(3)` view.

### Offline / mock-LLM end-to-end

For CI and air-gapped dev loops:

```bash
MTY_AGENT_SMOKE=1 bash demos/07_research_agent/smoke.sh
```

That starts the 60-line Python mock LLM server on `localhost:8775`,
points the demo at it via `ANTHROPIC_BASE_URL=http://localhost:8775`,
runs the agent against the canned response, and asserts stdout
contains the expected reply marker — exercising the whole pipeline
(vector index + spawn + memory bookkeeping + LLM round-trip) without
burning real API tokens.

## What each Track contributes

| Track | Surface | Where it lights up in this demo |
|---|---|---|
| A — `std.llm` | `client.messages(model, system, prompt, max_tokens)` | inside `on Ask` — the one-shot Anthropic call |
| B — `@tool` + `std.mcp` | `@tool("desc", cap: fs.read) fn read_doc(...)` | tool fns at the top of the file; cap-set enforced by `mty_stdlib::mcp::CapabilitySet` |
| C — `std.memory` | `VectorStore.local`, `Episodic.in_memory`, `Working.new` | constructed in `main`, passed to the agent as ctor args |

## Release context

This demo is the Track E close-out for v0.26 — the
"can-Mighty-actually-build-an-agent" forcing function. The earlier
v0.26 tracks shipped the surfaces; this demo proves they compose
into a working agent loop end-to-end. See
[`CHANGELOG.md`](../../CHANGELOG.md) under the v0.26 heading for the
full release notes.

## Prior demos

| Demo | Theme | What it proved |
|---|---|---|
| [01_search_api](../01_search_api/) | Backend service shape | spawn + protocol + bounded mailbox |
| [02_counter_web](../02_counter_web/) | Frontend agent | DOM ops via WIT |
| [03_extract_tool](../03_extract_tool/) | CLI tool | `mty new --template cli` |
| [04_kvstore](../04_kvstore/) | State + persistence | snapshot/restore |
| [05_notetris_web](../05_notetris_web/) | Browser game | `mty:web/canvas@0.1` |
| [06_canvas_game](../06_canvas_game/) | Canvas-direct rendering | Track A wasm32-web emitter completion |
| **07_research_agent** | **LLM-driven agent** | **v0.26 — std.llm + std.memory + @tool** |
