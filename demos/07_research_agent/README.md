# Demo 07 — Research Agent

The first **LLM-driven agent** demo in Mighty. A single ~200-LOC
`.mty` file that spawns a `Researcher` agent which indexes a text
corpus into a local vector store, answers questions over a typed
protocol mailbox, and persists turns into an episodic memory ring.
This is the forcing-function demo for Mighty's v0.26 LLM-agent
stack.

## What this demonstrates

| Surface | What this demo does |
|---|---|
| `std.llm` Anthropic provider | `AnthropicClient.complete(model, system, prompt, max_tokens)` inside the `on Ask` handler — one one-shot call per turn. |
| `std.memory.VectorStore` | `VectorStore.local(...)` indexes 5 corpus files; `search(query, k)` returns the top-K docs for the prompt builder. |
| `std.memory.Working` | Per-turn scratchpad; staged docs cleared each turn. |
| `std.memory.Episodic` | Ring buffer of (question, answer); the agent's `recent(3)` view threads prior turns into the next prompt. |
| Typed protocol mailbox | `protocol Research { Ask(q: Str) -> Str }` — return type reaches the call site. |
| `@tool` source-level decorator | `@tool("desc", cap: fs.read) fn read_doc(path)` — descriptor syntax shipped in v0.27 (Track B parser hook). |

Brought to its current shape by **v0.26** (the three v0.26 tracks
that ship `std.llm`, `std.memory`, and the `@tool` macro registry
all light up in this single file). Demo 08 builds on the same
stack to add multi-provider consensus.

## Layout

```
demos/07_research_agent/
├── README.md
├── mighty.toml                       # standard host-profile package
├── smoke.sh                          # check + fmt + opt-in mock-LLM end-to-end
├── src/main.mty                      # the Researcher agent (~200 LOC)
├── tools/
│   ├── sample_corpus/                # 5 short .txt files the agent indexes
│   │   ├── agent_features.txt
│   │   ├── mighty_overview.txt
│   │   ├── release_history.txt
│   │   ├── roadmap_post_v1.txt
│   │   └── stdlib_quick_ref.txt
│   └── mock_llm/server.py            # 60-line stub for offline smoke
└── answers/                          # save_answer() target (created on demand)
```

## Run

### Sanity check (no LLM call)

```bash
./target/debug/mty check demos/07_research_agent/src/main.mty
./target/debug/mty fmt --check demos/07_research_agent/src/main.mty
bash demos/07_research_agent/smoke.sh
```

The default smoke parses + checks + formats the source and runs
the sample-corpus indexer; no LLM key required.

### With a real Anthropic key

```bash
cargo build -p mty-cli
export ANTHROPIC_API_KEY=sk-ant-...
./target/debug/mty run demos/07_research_agent/src/main.mty
```

The first line of output is the indexing log:

```
research_agent: indexed sample corpus into ./tools/vector.json
```

After Claude returns, the second line is the answer. `Episodic`
records the question + answer for the next turn; re-running with a
follow-up question (the v0.26 demo only does one turn — multi-turn
is a v0.27 follow-up) will see the previous turn in its `recent(3)`
view.

The seed question (`"What does std.memory provide in Mighty
v0.26?"`) is hard-coded in `main()` because `mty run <path>` did
not yet accept `-- <argv>` positional forwarding into
`std.env.args()` at v0.26. Demos 08 + 09 use the v0.27 `-- argv`
plumbing for command-line snippets.

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

## What each v0.26 Track contributes

| Track | Surface | Where it lights up |
|---|---|---|
| A — `std.llm` | `client.complete(model, system, prompt, max_tokens)` | inside `on Ask` — the one-shot Anthropic call |
| B — `@tool` + `std.mcp` | `@tool("desc", cap: fs.read) fn read_doc(...)` | tool fns at the top of the file; cap-set enforced by `mty_stdlib::mcp::CapabilitySet` |
| C — `std.memory` | `VectorStore.local`, `Episodic.in_memory`, `Working.new` | constructed in `main`, passed to the agent as ctor args |

## Agent-mode usage (v0.33 T5)

The v0.33 T5 `mty agent` CLI exposes every other `mty` subcommand
over a structured JSON-over-stdio protocol. Once you've poked at the
research agent by hand, this is how an LLM agent (or CI script,
or VS Code panel) would drive it without scraping human output.

### One-shot check

```bash
echo '{"op":"check","path":"demos/07_research_agent/src/main.mty"}' \
  | ./target/debug/mty agent --single-shot
```

Streams one `kind:"envelope"` per diagnostic + a `kind:"result"`
summary + a `kind:"done"` terminator.

### One-shot explain a code

```bash
echo '{"op":"explain","code":"MT4099"}' \
  | ./target/debug/mty agent --single-shot
```

### Interactive session — check then fix

```bash
./target/debug/mty agent <<'EOF'
{"op":"check","path":"demos/07_research_agent/src/main.mty"}
{"op":"fix","code":"MT4099","alternative":0,"write":false}
{"op":"halt"}
EOF
```

The loop maintains in-memory state: after the `check` op, the next
`fix` op already knows which file + which envelope to target.
`write:false` (the default) previews the diff via `kind:"patch"`
without mutating the file; flip to `write:true` to apply.

### Substring search

```bash
echo '{"op":"find","query":"VectorStore","root":"demos/07_research_agent","top":5}' \
  | ./target/debug/mty agent --single-shot
```

Returns up to 5 `(file, line, text)` hits in `result.hits`. v0.34
wires this to the v0.33 T1 RAG index.

### Run the program

```bash
echo '{"op":"run","path":"demos/07_research_agent/src/main.mty"}' \
  | ./target/debug/mty agent --single-shot
```

### Spec + reference

* Wire format: `docs/internals/agent-mode-protocol.md`
* Human-facing CLI: `docs/reference/cli/mty-agent.md`
* Structured-diagnostics foundation (T4): `docs/internals/diagnostic-envelopes.md`

## Prior demos for context

| Demo | Theme | What it proved |
|---|---|---|
| [01_search_api](../01_search_api/) | Backend service shape | spawn + protocol + bounded mailbox |
| [02_counter_web](../02_counter_web/) | Frontend agent | DOM ops via WIT |
| [03_extract_tool](../03_extract_tool/) | CLI tool | sandbox + capability allow-list |
| [04_kvstore](../04_kvstore/) | State + persistence | supervisor + restart + crash-recovery |
| [05_notetris_web](../05_notetris_web/) | Browser game (log-driven) | `mty:web/log` import |
| [06_canvas_game](../06_canvas_game/) | Browser game (canvas-direct) | `mty:web/canvas@0.1` |
| **07_research_agent** | **LLM-driven agent** | **v0.26 — `std.llm` + `std.memory` + `@tool`** |
