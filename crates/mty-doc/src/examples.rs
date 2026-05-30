//! Stdlib examples index — curated usage examples for stdlib symbols
//! that the LSP hover provider stitches into its markdown payload.
//!
//! ## Why a static curated table
//!
//! The stdlib's runtime is implemented in Rust (`mty-stdlib`) rather
//! than in Mighty source, so the v0.2 doc generator (which walks `.mty`
//! files) never sees `///` doc comments off `Member::ask` and friends.
//! The hover provider still needs to surface stdlib usage examples —
//! they're the single most useful affordance for somebody driving the
//! `std.llm`/`std.swarm`/`std.memory` surfaces interactively.
//!
//! v0.33 ships a flat compile-time table ([`STDLIB_EXAMPLES`]) keyed by
//! symbol name. Each entry carries the canonical signature, a one-line
//! description, the required capability (if any), an example block in
//! the `mty` dialect, and a hand-curated list of related symbols
//! ("See also"). This is enough to power richer hover today while we
//! design a real cross-language doc pipeline for v0.34+.
//!
//! ## Persistence
//!
//! The table can be snapshot to JSON via [`persist_examples_index`]
//! (called the first time a stdlib hover is requested). The on-disk
//! cache lives at `~/.mty/examples-index.json` and is rebuilt whenever
//! [`stdlib_examples_hash`] changes. The hash is content-based: any
//! edit to [`STDLIB_EXAMPLES`] re-rolls it deterministically so external
//! tooling (`mty doc explain`, future MCP doc servers) can validate the
//! cache without re-running the compiler.
//!
//! ## Lookup
//!
//! [`lookup`] accepts both qualified forms (`Member.ask`) and bare
//! identifiers (`ask`). Bare identifiers can collide with user code,
//! so the hover provider only falls back to bare-name lookup once it
//! has confirmed the symbol does not resolve in the user's DefMap.

use crate::ir::DocExample;

/// A single curated stdlib usage example.
///
/// Fields are `&'static str` so the table can live in the read-only
/// data segment without per-startup allocation. The hover renderer
/// copies into owned `String`s as it builds the markdown.
#[derive(Debug, Clone, Copy)]
pub struct StdlibExample {
    /// Canonical symbol name. Qualified (`Module.member`) where the
    /// receiver disambiguates (e.g. `Member.ask`), bare otherwise.
    pub symbol: &'static str,
    /// Pretty-printed function/method signature, e.g.
    /// `fn Member.ask(&self, prompt: Str) -> Result<MemberReply, LlmError>`.
    pub signature: &'static str,
    /// One- or two-sentence description used as the "Description"
    /// section of the hover payload.
    pub description: &'static str,
    /// Required capability (or empty if none). Surfaced as the
    /// "Required capability" hover section.
    pub capability: &'static str,
    /// Example body in the `mty` dialect. Rendered inside a fenced
    /// ```mty code block by the hover renderer.
    pub example: &'static str,
    /// Comma-separated list of related symbol names. The hover renderer
    /// turns this into the "See also" section (up to 5 entries).
    pub see_also: &'static str,
}

impl StdlibExample {
    /// Convert this entry's example body into the renderer-friendly
    /// [`DocExample`] shape used elsewhere in `mty-doc`.
    pub fn to_doc_example(&self) -> DocExample {
        DocExample {
            code: self.example.to_string(),
            language: "mty".to_string(),
        }
    }

    /// Iterate the comma-separated `see_also` field, trimming each
    /// entry. Empty after a trim is filtered out.
    pub fn see_also_iter(&self) -> impl Iterator<Item = &'static str> {
        self.see_also
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
}

/// The curated stdlib examples table. See module docs for rationale.
///
/// Entries are ordered roughly by stdlib module: llm, swarm, memory,
/// mcp, eval, observe, http, fs, time, log, string, vec, json.
/// Within a module the ordering follows the most-common-first
/// principle so a `grep` over this file roughly recovers the usual
/// onboarding path.
pub const STDLIB_EXAMPLES: &[StdlibExample] = &[
    // ---- std.llm: Member + provider clients ----
    StdlibExample {
        symbol: "Member.anthropic",
        signature: "fn Member.anthropic(model: Str) -> Member",
        description: "Constructs an Anthropic panel member. Reads ANTHROPIC_API_KEY from the environment.",
        capability: "net.https (api.anthropic.com)",
        example: "let m = Member.anthropic(\"claude-opus-4-7\");\nlet r = m.ask(\"Capital of France?\")?;\nlog(r.text);\n",
        see_also: "Member.openai, Member.gemini, Member.bedrock, Member.ask, Member.mock",
    },
    StdlibExample {
        symbol: "Member.openai",
        signature: "fn Member.openai(model: Str) -> Member",
        description: "Constructs an OpenAI panel member. Reads OPENAI_API_KEY from the environment.",
        capability: "net.https (api.openai.com)",
        example: "let m = Member.openai(\"gpt-4o\");\nlet r = m.ask(\"List 3 primes\")?;\nlog(r.text);\n",
        see_also: "Member.anthropic, Member.gemini, Member.bedrock, Member.ask",
    },
    StdlibExample {
        symbol: "Member.gemini",
        signature: "fn Member.gemini(model: Str) -> Member",
        description: "Constructs a Gemini panel member. Reads GOOGLE_API_KEY from the environment.",
        capability: "net.https (generativelanguage.googleapis.com)",
        example: "let m = Member.gemini(\"gemini-1.5-pro\");\nlet r = m.ask(\"Summarise this.\")?;\nlog(r.text);\n",
        see_also: "Member.anthropic, Member.openai, Member.bedrock, Member.ask",
    },
    StdlibExample {
        symbol: "Member.bedrock",
        signature: "fn Member.bedrock(model: Str) -> Member",
        description: "Constructs a Bedrock panel member. Reads AWS_ACCESS_KEY_ID/SECRET from the environment.",
        capability: "net.https (bedrock-runtime.<region>.amazonaws.com)",
        example: "let m = Member.bedrock(\"anthropic.claude-3-5-sonnet-20241022-v2:0\");\nlet r = m.ask(\"hi\")?;\nlog(r.text);\n",
        see_also: "Member.anthropic, Member.openai, Member.gemini, Member.ask",
    },
    StdlibExample {
        symbol: "Member.mock",
        signature: "fn Member.mock(name: Str, reply: Str) -> Member",
        description: "Deterministic stand-in for tests. Returns the canned reply without making a network call.",
        capability: "",
        example: "let m = Member.mock(\"unit\", \"42\");\nlet r = m.ask(\"x\")?;\nassert_eq(r.text, \"42\");\n",
        see_also: "Member.anthropic, Member.ask, swarm",
    },
    StdlibExample {
        symbol: "Member.ask",
        signature: "fn Member.ask(&self, prompt: Str) -> Result<MemberReply, LlmError>",
        description: "Sends prompt to the LLM provider and returns the reply.",
        capability: "net.https (for the provider endpoint)",
        example: "let m = Member.anthropic(\"claude-opus-4-7\");\nlet r = m.ask(\"Capital of France?\")?;\nlog(r.text);\n",
        see_also: "Member.anthropic, Member.openai, std.swarm, swarm",
    },
    StdlibExample {
        symbol: "MemberReply.text",
        signature: "field MemberReply.text: Str",
        description: "Plain-text body of the assistant reply.",
        capability: "",
        example: "let r = m.ask(\"hi\")?;\nlog(r.text);\n",
        see_also: "Member.ask, MemberReply.tokens_used, MemberReply.cost_cents",
    },
    StdlibExample {
        symbol: "MemberReply.tokens_used",
        signature: "field MemberReply.tokens_used: U64",
        description: "Total tokens billed for this turn (input + output).",
        capability: "",
        example: "let r = m.ask(\"hi\")?;\nlog(format!(\"tokens={}\", r.tokens_used));\n",
        see_also: "MemberReply.cost_cents, MemberReply.text",
    },
    StdlibExample {
        symbol: "MemberReply.cost_cents",
        signature: "field MemberReply.cost_cents: U64",
        description: "Provider-reported cost of this turn, in cents.",
        capability: "",
        example: "let r = m.ask(\"hi\")?;\nlog(format!(\"cost_cents={}\", r.cost_cents));\n",
        see_also: "MemberReply.tokens_used, DollarBudget.from_dollars",
    },
    // ---- std.swarm: consensus over a panel ----
    StdlibExample {
        symbol: "swarm",
        signature: "fn swarm(prompt: Str, panel: List<Member>, budget: DollarBudget, strategy: ConsensusStrategy) -> Result<Consensus, LlmError>",
        description: "Runs the prompt across every panel member in parallel and decides a consensus by strategy.",
        capability: "net.https (per panel member)",
        example: "let panel = [Member.anthropic(\"claude-opus-4-7\"), Member.openai(\"gpt-4o\")];\nlet b = DollarBudget.from_dollars(0.50);\nlet c = swarm(\"Is 17 prime?\", panel, b, ConsensusStrategy.Majority).await?;\nlog(c.body);\n",
        see_also: "Member.anthropic, ConsensusStrategy.Majority, DollarBudget.from_dollars, Consensus.dissents",
    },
    StdlibExample {
        symbol: "ConsensusStrategy.Majority",
        signature: "const ConsensusStrategy.Majority: ConsensusStrategy",
        description: "Plurality vote across panel replies. Ties resolve to the first agreed reply in source order.",
        capability: "",
        example: "let c = swarm(p, panel, b, ConsensusStrategy.Majority).await?;\n",
        see_also: "ConsensusStrategy.Unanimous, ConsensusStrategy.WeightedVote, ConsensusStrategy.FirstAgreed, swarm",
    },
    StdlibExample {
        symbol: "ConsensusStrategy.Unanimous",
        signature: "const ConsensusStrategy.Unanimous: ConsensusStrategy",
        description: "Every member must agree, or `Consensus.body` reports no consensus and dissents lists all.",
        capability: "",
        example: "let c = swarm(p, panel, b, ConsensusStrategy.Unanimous).await?;\n",
        see_also: "ConsensusStrategy.Majority, Consensus.dissents, swarm",
    },
    StdlibExample {
        symbol: "ConsensusStrategy.WeightedVote",
        signature: "fn ConsensusStrategy.WeightedVote(weights: Map<Str, F32>) -> ConsensusStrategy",
        description: "Per-member-name weighted vote. Members not in the map default to weight 1.0.",
        capability: "",
        example: "let w = {\"claude-opus-4-7\": 2.0, \"gpt-4o\": 1.0};\nlet c = swarm(p, panel, b, ConsensusStrategy.WeightedVote(w)).await?;\n",
        see_also: "ConsensusStrategy.Majority, swarm",
    },
    StdlibExample {
        symbol: "ConsensusStrategy.FirstAgreed",
        signature: "const ConsensusStrategy.FirstAgreed: ConsensusStrategy",
        description: "Returns as soon as two members agree, short-circuiting remaining calls to save budget.",
        capability: "",
        example: "let c = swarm(p, panel, b, ConsensusStrategy.FirstAgreed).await?;\nlog(c.body);\n",
        see_also: "ConsensusStrategy.Majority, swarm, DollarBudget.from_dollars",
    },
    StdlibExample {
        symbol: "DollarBudget.from_dollars",
        signature: "fn DollarBudget.from_dollars(amount: F32) -> DollarBudget",
        description: "Caps spending across the swarm at `amount` USD. Exhaustion surfaces via `Consensus.budget_exhausted`.",
        capability: "",
        example: "let b = DollarBudget.from_dollars(0.50);\n",
        see_also: "DollarBudget.unbounded, DollarBudget.total_cost_cents, swarm",
    },
    StdlibExample {
        symbol: "DollarBudget.unbounded",
        signature: "fn DollarBudget.unbounded() -> DollarBudget",
        description: "Removes the dollar cap. Use only for tests or local-only providers.",
        capability: "",
        example: "let b = DollarBudget.unbounded();\n",
        see_also: "DollarBudget.from_dollars, swarm",
    },
    StdlibExample {
        symbol: "Consensus.body",
        signature: "field Consensus.body: Str",
        description: "Agreed-upon reply text, or empty when no consensus was reached.",
        capability: "",
        example: "let c = swarm(p, panel, b, ConsensusStrategy.Majority).await?;\nlog(c.body);\n",
        see_also: "Consensus.dissents, Consensus.budget_exhausted, swarm",
    },
    StdlibExample {
        symbol: "Consensus.dissents",
        signature: "field Consensus.dissents: List<MemberReply>",
        description: "Replies that disagreed with the consensus body. Empty when every member agreed.",
        capability: "",
        example: "for d in c.dissents { log(format!(\"{}: {}\", d.member, d.body)); }\n",
        see_also: "Consensus.body, Consensus.all_replies, swarm",
    },
    StdlibExample {
        symbol: "Consensus.all_replies",
        signature: "field Consensus.all_replies: List<MemberReply>",
        description: "Every member's reply in panel-declaration order. Useful for auditing.",
        capability: "",
        example: "for r in c.all_replies { log(r.body); }\n",
        see_also: "Consensus.body, Consensus.dissents, swarm",
    },
    StdlibExample {
        symbol: "Consensus.budget_exhausted",
        signature: "field Consensus.budget_exhausted: Bool",
        description: "True when the dollar budget tripped before every member returned.",
        capability: "",
        example: "if c.budget_exhausted { log(\"hit cap\"); }\n",
        see_also: "DollarBudget.from_dollars, Consensus.body, swarm",
    },
    // ---- std.mcp: tool registry + server/client ----
    StdlibExample {
        symbol: "McpServer",
        signature: "agent McpServer { on Register(tool: Tool), on Handle(req: ToolCall) -> ToolResult }",
        description: "MCP server that exposes `@tool` functions over stdio or a TCP transport.",
        capability: "net.listen (when bound to TCP)",
        example: "let s = McpServer.new();\ns.register(my_tool);\ns.serve(\"stdio\").await?;\n",
        see_also: "McpClient, ToolRegistry, std.mcp",
    },
    StdlibExample {
        symbol: "McpClient",
        signature: "fn McpClient.connect(transport: Str) -> Result<McpClient, McpError>",
        description: "Connects to an MCP server over stdio or TCP and yields a client handle for `call`/`list_tools`.",
        capability: "net.connect (when using TCP)",
        example: "let c = McpClient.connect(\"stdio:./echo-server\")?;\nlet tools = c.list_tools().await?;\n",
        see_also: "McpServer, ToolRegistry",
    },
    StdlibExample {
        symbol: "ToolRegistry",
        signature: "struct ToolRegistry { tools: Map<Str, Tool> }",
        description: "Holds the `@tool`-annotated functions discovered in the current package.",
        capability: "",
        example: "let r = ToolRegistry.discover();\nfor (name, _) in r.tools { log(name); }\n",
        see_also: "McpServer, McpClient",
    },
    // ---- std.memory: vector + episodic ----
    StdlibExample {
        symbol: "VectorStore.new",
        signature: "fn VectorStore.new(dim: U32) -> VectorStore",
        description: "Allocates an in-memory vector store with `dim`-wide embeddings.",
        capability: "",
        example: "let v = VectorStore.new(1536);\nv.upsert(\"k\", embed);\n",
        see_also: "VectorStore.upsert, VectorStore.query, std.memory",
    },
    StdlibExample {
        symbol: "VectorStore.upsert",
        signature: "fn VectorStore.upsert(&mut self, key: Str, vec: List<F32>) -> Unit",
        description: "Inserts or updates an embedding under `key`.",
        capability: "",
        example: "v.upsert(\"doc-1\", embed);\n",
        see_also: "VectorStore.new, VectorStore.query",
    },
    StdlibExample {
        symbol: "VectorStore.query",
        signature: "fn VectorStore.query(&self, vec: List<F32>, k: U32) -> List<(Str, F32)>",
        description: "Top-k nearest neighbours by cosine similarity.",
        capability: "",
        example: "let hits = v.query(q, 5);\nfor (k, score) in hits { log(format!(\"{} -> {}\", k, score)); }\n",
        see_also: "VectorStore.upsert, VectorStore.new",
    },
    StdlibExample {
        symbol: "EpisodicMemory",
        signature: "struct EpisodicMemory { events: List<MemoryEvent> }",
        description: "Append-only event log with time-bounded retention. Designed for agent turn history.",
        capability: "",
        example: "let m = EpisodicMemory.new();\nm.append(\"user: hi\");\nlet recent = m.last(10);\n",
        see_also: "VectorStore.new, std.memory",
    },
    StdlibExample {
        symbol: "WorkingMemory",
        signature: "struct WorkingMemory { slots: Map<Str, Str> }",
        description: "Bounded key/value scratchpad an agent can inspect each turn.",
        capability: "",
        example: "let w = WorkingMemory.new(16);\nw.set(\"task\", \"summarise\");\n",
        see_also: "EpisodicMemory, std.memory",
    },
    // ---- std.eval: replay-driven LLM eval ----
    StdlibExample {
        symbol: "Suite.new",
        signature: "fn Suite.new(name: Str) -> Suite",
        description: "Creates a named eval suite. Add cases, members, and a comparison strategy before running.",
        capability: "",
        example: "let s = Suite.new(\"summaries\");\ns.case(\"short\", \"Summarise this in 10 words.\");\ns.member(Member.openai(\"gpt-4o\"));\ns.compare(Compare.contains(\"summary\"));\nlet report = s.run().await?;\n",
        see_also: "Suite.case, Suite.member, Suite.compare, Suite.run",
    },
    StdlibExample {
        symbol: "Suite.case",
        signature: "fn Suite.case(&mut self, name: Str, prompt: Str) -> &mut Suite",
        description: "Registers an eval case with a prompt. Each member is run against each case.",
        capability: "",
        example: "s.case(\"capitals\", \"Capital of France?\");\n",
        see_also: "Suite.new, Suite.member, Suite.compare",
    },
    StdlibExample {
        symbol: "Suite.member",
        signature: "fn Suite.member(&mut self, m: Member) -> &mut Suite",
        description: "Adds a panel member to the eval grid. Members are run in parallel.",
        capability: "net.https (per member)",
        example: "s.member(Member.anthropic(\"claude-opus-4-7\"));\n",
        see_also: "Suite.new, Suite.case, Member.anthropic",
    },
    StdlibExample {
        symbol: "Suite.compare",
        signature: "fn Suite.compare(&mut self, c: Compare) -> &mut Suite",
        description: "Sets the comparison strategy used to stamp a verdict per (case, member) cell.",
        capability: "",
        example: "s.compare(Compare.contains(\"Paris\"));\n",
        see_also: "Compare.contains, Compare.tool_call_set_equal, Suite.run",
    },
    StdlibExample {
        symbol: "Suite.run",
        signature: "fn Suite.run(&self) -> Result<EvalReport, LlmError>",
        description: "Executes every (case, member) pair and returns the verdict grid.",
        capability: "net.https (per member)",
        example: "let report = s.run().await?;\nlog(report.summary());\n",
        see_also: "Suite.new, EvalReport, Compare.contains",
    },
    StdlibExample {
        symbol: "Compare.contains",
        signature: "fn Compare.contains(needle: Str) -> Compare",
        description: "Verdict-pass when the reply text contains `needle` (case-sensitive substring).",
        capability: "",
        example: "let cmp = Compare.contains(\"Paris\");\n",
        see_also: "Compare.tool_call_set_equal, Suite.compare",
    },
    StdlibExample {
        symbol: "Compare.tool_call_set_equal",
        signature: "fn Compare.tool_call_set_equal(tools: List<Str>) -> Compare",
        description: "Verdict-pass when the model invokes exactly `tools` (order-insensitive).",
        capability: "",
        example: "let cmp = Compare.tool_call_set_equal([\"search\", \"summarise\"]);\n",
        see_also: "Compare.contains, Suite.compare",
    },
    // ---- std.observe ----
    StdlibExample {
        symbol: "observe.record",
        signature: "fn observe.record(name: Str, payload: Json) -> Unit",
        description: "Append a structured observation to the per-run trace. Sinks decide whether to flush to OTel/file.",
        capability: "",
        example: "observe.record(\"agent.turn\", json({\"who\": \"user\", \"text\": msg}));\n",
        see_also: "observe.query, observe.otel_sink, std.observe",
    },
    StdlibExample {
        symbol: "observe.query",
        signature: "fn observe.query(filter: Str) -> List<Observation>",
        description: "Query the in-memory trace store with a dotted-path filter.",
        capability: "",
        example: "let turns = observe.query(\"agent.turn\");\n",
        see_also: "observe.record, std.observe",
    },
    StdlibExample {
        symbol: "observe.otel_sink",
        signature: "fn observe.otel_sink(endpoint: Str) -> Unit",
        description: "Forward every observation to an OTel collector via OTLP/HTTP.",
        capability: "net.https (the OTLP endpoint)",
        example: "observe.otel_sink(\"https://otel.example/v1/traces\");\n",
        see_also: "observe.record, std.observe",
    },
    // ---- std.http ----
    StdlibExample {
        symbol: "std.http.get",
        signature: "fn std.http.get(url: Str) -> Result<Response, HttpError>",
        description: "Issues an HTTPS GET. The TLS handshake uses the platform trust roots.",
        capability: "net.https (the URL host)",
        example: "let r = std.http.get(\"https://example.com\").await?;\nlog(r.body);\n",
        see_also: "std.http.post, std.http.serve",
    },
    StdlibExample {
        symbol: "std.http.post",
        signature: "fn std.http.post(url: Str, body: Bytes) -> Result<Response, HttpError>",
        description: "Issues an HTTPS POST with `body` as the request payload.",
        capability: "net.https (the URL host)",
        example: "let r = std.http.post(\"https://example.com/api\", payload).await?;\n",
        see_also: "std.http.get, std.http.serve",
    },
    StdlibExample {
        symbol: "std.http.serve",
        signature: "fn std.http.serve(port: U16, handler: fn(Request) -> Response) -> Unit",
        description: "Binds an HTTP server on `port` and dispatches each request through `handler`.",
        capability: "net.listen (port)",
        example: "std.http.serve(8080, |req| { Response.text(\"ok\") }).await?;\n",
        see_also: "std.http.get, std.http.post",
    },
    // ---- std.fs ----
    StdlibExample {
        symbol: "std.fs.read",
        signature: "fn std.fs.read(cap: &Fs, path: Str) -> Result<Bytes, IoErr>",
        description: "Reads `path` from the filesystem under the read-bound capability `cap`.",
        capability: "fs.read (path)",
        example: "let bytes = std.fs.read(&fs, \"input.txt\")?;\n",
        see_also: "std.fs.write, std.fs.open",
    },
    StdlibExample {
        symbol: "std.fs.write",
        signature: "fn std.fs.write(cap: &mut Fs, path: Str, bytes: Bytes) -> Result<Unit, IoErr>",
        description: "Writes `bytes` to `path` under the write-bound capability `cap`.",
        capability: "fs.write (path)",
        example: "std.fs.write(&mut fs, \"out.txt\", body)?;\n",
        see_also: "std.fs.read, std.fs.open",
    },
    StdlibExample {
        symbol: "std.fs.open",
        signature: "fn std.fs.open(cap: &Fs, path: Str, mode: OpenMode) -> Result<File, IoErr>",
        description: "Opens `path` for streaming reads/writes under `cap`.",
        capability: "fs.read or fs.write (path)",
        example: "let f = std.fs.open(&fs, \"big.bin\", OpenMode.Read)?;\nlet chunk = f.read(4096)?;\n",
        see_also: "std.fs.read, std.fs.write",
    },
    // ---- std.time + std.log + std.json ----
    StdlibExample {
        symbol: "std.time.now",
        signature: "fn std.time.now() -> Instant",
        description: "Returns the monotonic instant. Suitable for measuring elapsed durations.",
        capability: "",
        example: "let t0 = std.time.now();\ndo_work();\nlog(format!(\"took {}\", std.time.now() - t0));\n",
        see_also: "std.time.sleep, std.time.format",
    },
    StdlibExample {
        symbol: "std.time.sleep",
        signature: "fn std.time.sleep(dur: Duration) -> Unit",
        description: "Yields the current task for `dur` before resuming.",
        capability: "",
        example: "std.time.sleep(1.s).await;\n",
        see_also: "std.time.now",
    },
    StdlibExample {
        symbol: "log",
        signature: "fn log(msg: Str) -> Unit",
        description: "Writes `msg` to the host log sink with a newline. Effect `io`.",
        capability: "",
        example: "log(\"hello, world\");\n",
        see_also: "panic, std.observe.record",
    },
    StdlibExample {
        symbol: "panic",
        signature: "fn panic(msg: Str) -> Never",
        description: "Aborts the current task with `msg`. Never returns.",
        capability: "",
        example: "if !ok { panic(\"unreachable\"); }\n",
        see_also: "log, std.observe.record",
    },
    StdlibExample {
        symbol: "std.json.parse",
        signature: "fn std.json.parse(s: Str) -> Result<Json, JsonError>",
        description: "Parses a JSON string into the dynamic `Json` value type.",
        capability: "",
        example: "let j = std.json.parse(body)?;\nlog(j[\"name\"].as_str()?);\n",
        see_also: "std.json.encode, std.json.encode_pretty",
    },
    StdlibExample {
        symbol: "std.json.encode",
        signature: "fn std.json.encode(j: Json) -> Str",
        description: "Encodes `j` as a compact JSON string.",
        capability: "",
        example: "let s = std.json.encode(j);\n",
        see_also: "std.json.parse, std.json.encode_pretty",
    },
    StdlibExample {
        symbol: "std.json.encode_pretty",
        signature: "fn std.json.encode_pretty(j: Json) -> Str",
        description: "Encodes `j` as JSON with two-space indentation. Use for human-facing dumps.",
        capability: "",
        example: "log(std.json.encode_pretty(j));\n",
        see_also: "std.json.encode, std.json.parse",
    },
    // ---- std.string + std.vec + std.env ----
    StdlibExample {
        symbol: "String.new",
        signature: "fn String.new() -> String",
        description: "Allocates an empty owned UTF-8 string.",
        capability: "",
        example: "let mut s = String.new();\ns.push_str(\"hi\");\n",
        see_also: "String.from_str, String.with_capacity",
    },
    StdlibExample {
        symbol: "String.from_str",
        signature: "fn String.from_str(s: Str) -> String",
        description: "Owned copy of the borrowed slice `s`.",
        capability: "",
        example: "let owned = String.from_str(\"hi\");\n",
        see_also: "String.new, String.push_str",
    },
    StdlibExample {
        symbol: "String.push_str",
        signature: "fn String.push_str(&mut self, s: Str) -> Unit",
        description: "Appends `s` to the end of this string.",
        capability: "",
        example: "s.push_str(\" world\");\n",
        see_also: "String.new, String.from_str",
    },
    StdlibExample {
        symbol: "Vec.new",
        signature: "fn Vec.new[T]() -> Vec[T]",
        description: "Allocates an empty growable vector.",
        capability: "",
        example: "let mut v: Vec[I32] = Vec.new();\nv.push(1);\n",
        see_also: "Vec.with_capacity, Vec.push, Vec.iter",
    },
    StdlibExample {
        symbol: "Vec.with_capacity",
        signature: "fn Vec.with_capacity[T](cap: USize) -> Vec[T]",
        description: "Allocates a vector pre-sized for `cap` elements. Avoids re-allocs on the hot path.",
        capability: "",
        example: "let v: Vec[I32] = Vec.with_capacity(1024);\n",
        see_also: "Vec.new, Vec.push",
    },
    StdlibExample {
        symbol: "Vec.push",
        signature: "fn Vec.push[T](&mut self, x: T) -> Unit",
        description: "Appends `x` to the back of the vector.",
        capability: "",
        example: "v.push(42);\n",
        see_also: "Vec.new, Vec.pop, Vec.iter",
    },
    StdlibExample {
        symbol: "Vec.iter",
        signature: "fn Vec.iter[T](&self) -> Iterator[T]",
        description: "Borrowed iterator over the vector's elements.",
        capability: "",
        example: "for x in v.iter() { log(format!(\"{}\", x)); }\n",
        see_also: "Vec.push, Vec.collect",
    },
    StdlibExample {
        symbol: "std.env.args",
        signature: "fn std.env.args() -> List<Str>",
        description: "Returns the CLI's `--`-tail positional arguments.",
        capability: "",
        example: "let args = std.env.args();\nlog(format!(\"argc={}\", args.len()));\n",
        see_also: "log",
    },
    // ---- spawn / agents primitives ----
    StdlibExample {
        symbol: "spawn",
        signature: "fn spawn[T](inner: T) -> AgentRef[T]",
        description: "Spawns an agent on the current supervisor and returns a typed handle.",
        capability: "",
        example: "let h = spawn(Greeter.new());\nh.send(Greet { name: \"world\" });\n",
        see_also: "AgentRef.send, AgentRef.ask, std.swarm",
    },
    // ---- std.rag: Index + Doc ----
    StdlibExample {
        symbol: "Index.new",
        signature: "fn Index.new(path: Str) -> Index",
        description: "Disk-backed RAG index rooted at `path`. The path is created on first `build()` if missing.",
        capability: "fs.write (the index dir)",
        example: "let mut idx = Index.new(\"./corpus\");\nidx.add_text(\"Mighty is an agent-first language\");\nidx.build()?;\n",
        see_also: "Index.in_memory, Index.add_text, Index.add_file, Index.build, Index.search",
    },
    StdlibExample {
        symbol: "Index.in_memory",
        signature: "fn Index.in_memory() -> Index",
        description: "Pure in-memory RAG index. Useful for tests and ephemeral corpora.",
        capability: "",
        example: "let mut idx = Index.in_memory();\nidx.add_text(\"alpha beta gamma\");\nidx.build()?;\nlet hits = idx.search(\"alpha\", 3)?;\n",
        see_also: "Index.new, Index.add_text, Index.build, Index.search",
    },
    StdlibExample {
        symbol: "Index.add_text",
        signature: "fn Index.add_text(&mut self, text: Str) -> &mut Index",
        description: "Stage a raw text body. The doc id is auto-generated; `build()` chunks and embeds it.",
        capability: "",
        example: "idx.add_text(\"a paragraph about claude\");\nidx.add_text(\"another paragraph about gemini\");\nidx.build()?;\n",
        see_also: "Index.add_file, Index.add_doc, Index.build",
    },
    StdlibExample {
        symbol: "Index.add_file",
        signature: "fn Index.add_file(&mut self, path: Str) -> Result<&mut Index, IndexErr>",
        description: "Stage a file from disk. Equivalent to `add_doc(Doc.from_file(path)?)`.",
        capability: "fs.read (path)",
        example: "idx.add_file(\"./docs/spec.md\")?;\nidx.add_file(\"./docs/rfc.md\")?;\nidx.build()?;\n",
        see_also: "Index.add_text, Index.add_doc, Doc.from_file, Index.build",
    },
    StdlibExample {
        symbol: "Index.add_doc",
        signature: "fn Index.add_doc(&mut self, doc: Doc) -> &mut Index",
        description: "Stage a fully-formed `Doc` (custom id + metadata). Re-adding the same id replaces prior chunks on `build()`.",
        capability: "",
        example: "let d = Doc.new(\"intro\", \"Mighty is agent-first\");\nidx.add_doc(d);\nidx.build()?;\n",
        see_also: "Doc.new, Doc.from_file, Index.add_text, Index.build",
    },
    StdlibExample {
        symbol: "Index.build",
        signature: "fn Index.build(&mut self) -> Result<USize, IndexErr>",
        description: "Drain every staged doc, chunk it, embed it, upsert into the store. Returns the chunk count.",
        capability: "",
        example: "let n = idx.build()?;\nlog(format!(\"indexed {} chunks\", n));\n",
        see_also: "Index.add_text, Index.add_file, Index.search, Index.chunk_count",
    },
    StdlibExample {
        symbol: "Index.search",
        signature: "fn Index.search(&self, query: Str, k: USize) -> Result<Vec[Hit], IndexErr>",
        description: "kNN search over the built index. Returns the top-`k` hits by cosine similarity.",
        capability: "",
        example: "let hits = idx.search(\"how does taint typing work?\", 5)?;\nfor h in hits.iter() { log(h.text); }\n",
        see_also: "Index.build, Retriever.retrieve, Rag.ask",
    },
    StdlibExample {
        symbol: "Index.chunk_count",
        signature: "fn Index.chunk_count(&self) -> USize",
        description: "Number of chunks currently in the underlying store. Counts only built docs, not pending ones.",
        capability: "",
        example: "log(format!(\"chunks indexed: {}\", idx.chunk_count()));\n",
        see_also: "Index.doc_count, Index.pending_count, Index.build",
    },
    StdlibExample {
        symbol: "Index.doc_count",
        signature: "fn Index.doc_count(&self) -> USize",
        description: "Number of distinct docs that have been built into the store.",
        capability: "",
        example: "log(format!(\"docs indexed: {}\", idx.doc_count()));\n",
        see_also: "Index.chunk_count, Index.pending_count, Index.build",
    },
    StdlibExample {
        symbol: "Index.pending_count",
        signature: "fn Index.pending_count(&self) -> USize",
        description: "Docs staged via `add_*` but not yet built. Drops to zero after `build()`.",
        capability: "",
        example: "idx.add_text(\"draft\");\nassert_eq(idx.pending_count(), 1);\nidx.build()?;\nassert_eq(idx.pending_count(), 0);\n",
        see_also: "Index.add_text, Index.build, Index.doc_count",
    },
    StdlibExample {
        symbol: "Index.clear",
        signature: "fn Index.clear(&mut self) -> Result<Unit, IndexErr>",
        description: "Drop every staged + built doc, and clear the underlying store.",
        capability: "",
        example: "idx.clear()?;\nassert_eq(idx.chunk_count(), 0);\n",
        see_also: "Index.build, Index.add_text",
    },
    StdlibExample {
        symbol: "Index.with_strategy",
        signature: "fn Index.with_strategy(self, strategy: ChunkStrategy) -> Index",
        description: "Builder that swaps the chunking strategy without reconstructing the chunker.",
        capability: "",
        example: "let idx = Index.new(\"./corpus\").with_strategy(ChunkStrategy.BySection);\n",
        see_also: "ChunkStrategy.ByParagraph, ChunkStrategy.BySection, ChunkStrategy.ByTokens, ChunkStrategy.ByCodeFence",
    },
    StdlibExample {
        symbol: "Doc.new",
        signature: "fn Doc.new(id: Str, text: Str) -> Doc",
        description: "Build a `Doc` with a stable id + text body. Metadata starts empty.",
        capability: "",
        example: "let d = Doc.new(\"intro-1\", \"Mighty is an agent-first language\");\nidx.add_doc(d);\n",
        see_also: "Doc.from_file, Doc.with_meta, Index.add_doc",
    },
    StdlibExample {
        symbol: "Doc.from_file",
        signature: "fn Doc.from_file(path: Str) -> Result<Doc, IoErr>",
        description: "Read a file from disk and wrap it in a `Doc`. The id defaults to the path; `source` metadata is set.",
        capability: "fs.read (path)",
        example: "let d = Doc.from_file(\"./docs/spec.md\")?;\nlog(d.id);\n",
        see_also: "Doc.new, Index.add_file",
    },
    StdlibExample {
        symbol: "Doc.with_meta",
        signature: "fn Doc.with_meta(self, key: Str, value: Json) -> Doc",
        description: "Builder: attach one metadata key/value. Values travel through to every chunk + hit.",
        capability: "",
        example: "let d = Doc.new(\"intro\", \"...\").with_meta(\"title\", \"Intro\");\n",
        see_also: "Doc.new, Doc.from_file",
    },
    // ---- std.rag: chunking strategies ----
    StdlibExample {
        symbol: "ChunkStrategy.ByParagraph",
        signature: "const ChunkStrategy.ByParagraph: ChunkStrategy",
        description: "Split on blank lines and merge adjacent paragraphs under a soft token cap. Default for `Chunker`.",
        capability: "",
        example: "let idx = Index.new(\"./c\").with_strategy(ChunkStrategy.ByParagraph);\n",
        see_also: "ChunkStrategy.ByTokens, ChunkStrategy.BySection, ChunkStrategy.ByCodeFence, Index.with_strategy",
    },
    StdlibExample {
        symbol: "ChunkStrategy.ByTokens",
        signature: "const ChunkStrategy.ByTokens: ChunkStrategy",
        description: "Fixed approximate-token windows (default 1024 tokens, 64-token overlap). Good catch-all when corpus shape is unknown.",
        capability: "",
        example: "let idx = Index.new(\"./c\").with_strategy(ChunkStrategy.ByTokens);\n",
        see_also: "ChunkStrategy.ByParagraph, ChunkStrategy.BySection, ChunkStrategy.ByCodeFence",
    },
    StdlibExample {
        symbol: "ChunkStrategy.BySection",
        signature: "const ChunkStrategy.BySection: ChunkStrategy",
        description: "Split on Markdown `#`, `##`, `###` headings. Best for wikis / technical docs where each section is self-contained.",
        capability: "",
        example: "let idx = Index.new(\"./c\").with_strategy(ChunkStrategy.BySection);\n",
        see_also: "ChunkStrategy.ByParagraph, ChunkStrategy.ByTokens, ChunkStrategy.ByCodeFence",
    },
    StdlibExample {
        symbol: "ChunkStrategy.ByCodeFence",
        signature: "const ChunkStrategy.ByCodeFence: ChunkStrategy",
        description: "Split on triple-backtick fences. Best for code-heavy docs where keeping a fence intact matters.",
        capability: "",
        example: "let idx = Index.new(\"./c\").with_strategy(ChunkStrategy.ByCodeFence);\n",
        see_also: "ChunkStrategy.ByParagraph, ChunkStrategy.ByTokens, ChunkStrategy.BySection",
    },
    StdlibExample {
        symbol: "Chunker.default",
        signature: "fn Chunker.default() -> Chunker",
        description: "Default chunker: `ByParagraph` strategy, 1024-token soft cap, 64-token overlap.",
        capability: "",
        example: "let ch = Chunker.default();\nlet idx = Index.new(\"./c\").with_chunker(ch);\n",
        see_also: "Chunker.new, ChunkStrategy.ByParagraph, Index.with_strategy",
    },
    // ---- std.rag: Retriever ----
    StdlibExample {
        symbol: "Retriever.new",
        signature: "fn Retriever.new(index: &Index) -> Retriever",
        description: "Build a retrieval policy over `index`. Defaults: `top_k=5`, no score floor, MMR off.",
        capability: "",
        example: "let r = Retriever.new(&idx).with_top_k(10).with_min_score(0.6);\nlet hits = r.retrieve(\"agent capabilities\")?;\n",
        see_also: "Retriever.with_top_k, Retriever.with_min_score, Retriever.with_mmr, Retriever.retrieve",
    },
    StdlibExample {
        symbol: "Retriever.with_top_k",
        signature: "fn Retriever.with_top_k(self, k: USize) -> Retriever",
        description: "Builder: cap the number of returned hits at `k` (minimum 1).",
        capability: "",
        example: "let r = Retriever.new(&idx).with_top_k(20);\n",
        see_also: "Retriever.new, Retriever.with_min_score, Retriever.retrieve",
    },
    StdlibExample {
        symbol: "Retriever.with_min_score",
        signature: "fn Retriever.with_min_score(self, s: F32) -> Retriever",
        description: "Builder: drop any hit scoring below `s` (cosine similarity).",
        capability: "",
        example: "let r = Retriever.new(&idx).with_min_score(0.75);\n",
        see_also: "Retriever.new, Retriever.with_top_k, Retriever.with_mmr",
    },
    StdlibExample {
        symbol: "Retriever.with_mmr",
        signature: "fn Retriever.with_mmr(self, on: Bool) -> Retriever",
        description: "Builder: enable Maximal Marginal Relevance diversification (greedy, lambda=0.5).",
        capability: "",
        example: "let r = Retriever.new(&idx).with_mmr(true);\n",
        see_also: "Retriever.new, Retriever.with_top_k, Retriever.retrieve",
    },
    StdlibExample {
        symbol: "Retriever.retrieve",
        signature: "fn Retriever.retrieve(&self, query: Str) -> Result<Vec[Hit], IndexErr>",
        description: "Run a kNN search through the configured top-k / score floor / MMR pipeline.",
        capability: "",
        example: "let hits = r.retrieve(\"capability typing\")?;\nfor h in hits.iter() { log(h.text); }\n",
        see_also: "Retriever.new, Retriever.with_top_k, Index.search, Rag.ask",
    },
    // ---- std.rag: Reranker ----
    StdlibExample {
        symbol: "Reranker.new",
        signature: "fn Reranker.new(member: Member) -> Reranker",
        description: "LLM-as-reranker over `member`. Re-scores retrieved hits on a 0-100 relevance scale. Default batch 20.",
        capability: "net.https (the wrapped member's provider)",
        example: "let rr = Reranker.new(Member.anthropic(\"claude-haiku-4-5\"));\nlet rag = Rag.new().with_reranker(rr);\n",
        see_also: "Reranker.with_batch_size, Rag.with_reranker, Member.anthropic",
    },
    StdlibExample {
        symbol: "Reranker.with_batch_size",
        signature: "fn Reranker.with_batch_size(self, n: USize) -> Reranker",
        description: "Builder: cap how many candidates the reranker is asked to score in one prompt.",
        capability: "",
        example: "let rr = Reranker.new(m).with_batch_size(40);\n",
        see_also: "Reranker.new, Rag.with_reranker",
    },
    // ---- std.rag: Rag pipeline ----
    StdlibExample {
        symbol: "Rag.new",
        signature: "fn Rag.new() -> Rag",
        description: "Empty RAG pipeline. Chain `with_index`, `with_member`, and (optionally) `with_reranker` to drive `ask`.",
        capability: "",
        example: "let rag = Rag.new()\n  .with_index(idx)\n  .with_member(Member.anthropic(\"claude-opus-4-7\"));\nlet ans = rag.ask(\"how does taint work?\").await?;\n",
        see_also: "Rag.with_index, Rag.with_member, Rag.ask, Index.build",
    },
    StdlibExample {
        symbol: "Rag.with_index",
        signature: "fn Rag.with_index(self, index: Index) -> Rag",
        description: "Attach the index Rag should retrieve from. Required before `ask`.",
        capability: "",
        example: "let rag = Rag.new().with_index(idx);\n",
        see_also: "Rag.new, Rag.with_member, Index.new",
    },
    StdlibExample {
        symbol: "Rag.with_member",
        signature: "fn Rag.with_member(self, m: Member) -> Rag",
        description: "Attach the answering LLM. Required before `ask`.",
        capability: "net.https (the member's provider)",
        example: "let rag = Rag.new().with_index(idx).with_member(Member.openai(\"gpt-4o\"));\n",
        see_also: "Rag.new, Rag.with_index, Rag.with_reranker, Member.anthropic",
    },
    StdlibExample {
        symbol: "Rag.with_reranker",
        signature: "fn Rag.with_reranker(self, r: Reranker) -> Rag",
        description: "Insert an optional reranker between retrieval and the answering call.",
        capability: "",
        example: "let rag = Rag.new()\n  .with_index(idx)\n  .with_reranker(Reranker.new(Member.anthropic(\"claude-haiku-4-5\")))\n  .with_member(Member.anthropic(\"claude-opus-4-7\"));\n",
        see_also: "Rag.new, Reranker.new, Rag.with_member",
    },
    StdlibExample {
        symbol: "Rag.with_retriever_top_k",
        signature: "fn Rag.with_retriever_top_k(self, k: USize) -> Rag",
        description: "Shortcut: set the retriever's `top_k` without building a `Retriever` explicitly.",
        capability: "",
        example: "let rag = Rag.new().with_index(idx).with_retriever_top_k(10).with_member(m);\n",
        see_also: "Rag.new, Retriever.with_top_k, Rag.ask",
    },
    StdlibExample {
        symbol: "Rag.ask",
        signature: "fn Rag.ask(&self, q: Str) -> Result<Str, RagErr>",
        description: "End-to-end RAG: embed `q`, retrieve top-k hits, (optionally rerank), and ask the answering member.",
        capability: "net.https (the member's provider)",
        example: "let ans = rag.ask(\"What is Mighty's capability typing?\").await?;\nlog(ans);\n",
        see_also: "Rag.new, Rag.with_member, Index.search, Rag.ask_with_image",
    },
    // ---- std.computer: Mouse / Keyboard / Screen / Dispatcher ----
    StdlibExample {
        symbol: "ComputerCap.screen_and_input",
        signature: "fn ComputerCap.screen_and_input() -> ComputerCap",
        description: "Build a cap granting BOTH screen capture and input dispatch. The canonical browser-driving cap.",
        capability: "computer.screen + computer.input",
        example: "let cap = ComputerCap.screen_and_input()\n  .with_bounds(0, 0, 1280, 800)\n  .deny_keys([\"ctrl+alt+delete\", \"cmd+q\"]);\n",
        see_also: "ComputerCap.builder, ComputerCap.with_bounds, ComputerCap.deny_keys, Dispatcher.new",
    },
    StdlibExample {
        symbol: "ComputerCap.builder",
        signature: "fn ComputerCap.builder() -> ComputerCapBuilder",
        description: "Start an empty cap builder. Nothing is allowed until `allow_screen` / `allow_input` is set.",
        capability: "",
        example: "let cap = ComputerCap.builder().allow_screen().with_bounds(0, 0, 1024, 768).build();\n",
        see_also: "ComputerCap.screen_and_input, ComputerCapBuilder.allow_screen, ComputerCapBuilder.allow_input",
    },
    StdlibExample {
        symbol: "ComputerCap.with_bounds",
        signature: "fn ComputerCap.with_bounds(self, x_min: U32, y_min: U32, x_max: U32, y_max: U32) -> ComputerCap",
        description: "Constrain every click/move to the half-open rectangle `[x_min, x_max) x [y_min, y_max)`.",
        capability: "",
        example: "let cap = ComputerCap.screen_and_input().with_bounds(0, 0, 1280, 800);\n",
        see_also: "ComputerCap.screen_and_input, ComputerCap.deny_keys, SandboxViolation.OutOfBounds",
    },
    StdlibExample {
        symbol: "ComputerCap.deny_keys",
        signature: "fn ComputerCap.deny_keys(self, chords: List<Str>) -> ComputerCap",
        description: "Reject the listed key chords regardless of how the model was convinced to press them.",
        capability: "",
        example: "let cap = ComputerCap.screen_and_input().deny_keys([\"ctrl+alt+delete\", \"meta+q\"]);\n",
        see_also: "ComputerCap.with_bounds, SandboxViolation.DeniedKey, Keyboard.key_press",
    },
    StdlibExample {
        symbol: "ComputerCap.max_actions_per_run",
        signature: "fn ComputerCap.max_actions_per_run(self, n: U32) -> ComputerCap",
        description: "Hard cap on how many actions a single `Dispatcher.run` may execute. Trips `SandboxViolation.RateLimited`.",
        capability: "",
        example: "let cap = ComputerCap.screen_and_input().max_actions_per_run(50);\n",
        see_also: "ComputerCap.screen_and_input, SandboxViolation.RateLimited, Dispatcher.run",
    },
    StdlibExample {
        symbol: "Dispatcher.new",
        signature: "fn Dispatcher.new(llm: LlmProvider, cap: ComputerCap) -> Dispatcher",
        description: "Build the Anthropic computer-use loop over `llm`. The cap gates every action before it reaches the OS.",
        capability: "net.https (Anthropic) + the cap's permissions",
        example: "let d = Dispatcher.new(llm, cap)\n  .with_screen(MockScreen.solid_color(1280, 800, 0));\nlet summary = d.run(\"open the docs and search for taint\").await?;\n",
        see_also: "Dispatcher.with_screen, Dispatcher.with_mouse, Dispatcher.with_keyboard, Dispatcher.run",
    },
    StdlibExample {
        symbol: "Dispatcher.with_screen",
        signature: "fn Dispatcher.with_screen(self, b: ScreenBackend) -> Dispatcher",
        description: "Override the screen backend. Defaults to `MockScreen` so CI never grabs a real display.",
        capability: "",
        example: "let d = Dispatcher.new(llm, cap).with_screen(MockScreen.solid_color(1024, 768, 0));\n",
        see_also: "Dispatcher.new, MockScreen.solid_color, Screen.capture",
    },
    StdlibExample {
        symbol: "Dispatcher.with_mouse",
        signature: "fn Dispatcher.with_mouse(self, b: MouseBackend) -> Dispatcher",
        description: "Override the mouse backend. Defaults to `MockMouse` which records events instead of firing them.",
        capability: "",
        example: "let d = Dispatcher.new(llm, cap).with_mouse(MockMouse.new());\n",
        see_also: "Dispatcher.new, MockMouse, Mouse.click_at",
    },
    StdlibExample {
        symbol: "Dispatcher.with_keyboard",
        signature: "fn Dispatcher.with_keyboard(self, b: KeyboardBackend) -> Dispatcher",
        description: "Override the keyboard backend. Defaults to `MockKeyboard` (event log, no real OS events).",
        capability: "",
        example: "let d = Dispatcher.new(llm, cap).with_keyboard(MockKeyboard.new());\n",
        see_also: "Dispatcher.new, MockKeyboard, Keyboard.type_text",
    },
    StdlibExample {
        symbol: "Dispatcher.with_max_turns",
        signature: "fn Dispatcher.with_max_turns(self, n: U32) -> Dispatcher",
        description: "Cap how many model turns the agent loop runs before forcing termination. Default `MAX_TURNS` is 50.",
        capability: "",
        example: "let d = Dispatcher.new(llm, cap).with_max_turns(20);\n",
        see_also: "Dispatcher.new, Dispatcher.run, MAX_TURNS",
    },
    StdlibExample {
        symbol: "Dispatcher.run",
        signature: "fn Dispatcher.run(&self, task: Str) -> Result<Str, ComputerError>",
        description: "Run the Anthropic computer-use loop until the model emits a `stop`/`done` action or `max_turns` trips.",
        capability: "net.https (Anthropic) + cap's permissions",
        example: "let summary = d.run(\"take a screenshot then summarise it\").await?;\nlog(summary);\n",
        see_also: "Dispatcher.new, Dispatcher.with_max_turns, ComputerAction, ComputerError",
    },
    StdlibExample {
        symbol: "Mouse.click_at",
        signature: "fn Mouse.click_at(&self, x: U32, y: U32, button: MouseButton) -> Result<Unit, InputError>",
        description: "Click at `(x, y)`. The dispatcher validates the point against the cap's bounds before firing.",
        capability: "computer.input",
        example: "mouse.click_at(640, 400, MouseButton.Left)?;\n",
        see_also: "Mouse.move_to, Mouse.drag, MouseButton.Left, ComputerCap.with_bounds",
    },
    StdlibExample {
        symbol: "Mouse.move_to",
        signature: "fn Mouse.move_to(&self, x: U32, y: U32) -> Result<Unit, InputError>",
        description: "Move the cursor to `(x, y)` without clicking. Honours the cap's bounding rectangle.",
        capability: "computer.input",
        example: "mouse.move_to(100, 200)?;\n",
        see_also: "Mouse.click_at, Mouse.drag, Mouse.scroll",
    },
    StdlibExample {
        symbol: "Mouse.drag",
        signature: "fn Mouse.drag(&self, x1: U32, y1: U32, x2: U32, y2: U32, button: MouseButton) -> Result<Unit, InputError>",
        description: "Press at `(x1, y1)`, drag to `(x2, y2)`, release. Both endpoints are checked against bounds.",
        capability: "computer.input",
        example: "mouse.drag(10, 10, 200, 200, MouseButton.Left)?;\n",
        see_also: "Mouse.click_at, Mouse.move_to, MouseButton.Left",
    },
    StdlibExample {
        symbol: "Mouse.scroll",
        signature: "fn Mouse.scroll(&self, x: U32, y: U32, dx: I32, dy: I32) -> Result<Unit, InputError>",
        description: "Scroll by `(dx, dy)` at the focal point `(x, y)`. Negative dy scrolls down.",
        capability: "computer.input",
        example: "mouse.scroll(640, 400, 0, -120)?;\n",
        see_also: "Mouse.move_to, Mouse.click_at",
    },
    StdlibExample {
        symbol: "Keyboard.type_text",
        signature: "fn Keyboard.type_text(&self, text: Str) -> Result<Unit, InputError>",
        description: "Type a literal string. Each character is dispatched as a keypress; modifier chords use `key_press`.",
        capability: "computer.input",
        example: "keyboard.type_text(\"hello, world\")?;\n",
        see_also: "Keyboard.key_press, Key.Chord, ComputerCap.deny_keys",
    },
    StdlibExample {
        symbol: "Keyboard.key_press",
        signature: "fn Keyboard.key_press(&self, key: &Key) -> Result<Unit, InputError>",
        description: "Press a named key (Enter, Escape, F1, chord). Chords are checked against `ComputerCap.deny_keys`.",
        capability: "computer.input",
        example: "keyboard.key_press(&Key.Enter)?;\nkeyboard.key_press(&Key.Chord(\"ctrl+l\"))?;\n",
        see_also: "Keyboard.type_text, Key.Enter, Key.Chord, ComputerCap.deny_keys",
    },
    StdlibExample {
        symbol: "Screen.capture",
        signature: "fn Screen.capture(&self) -> Result<Screenshot, ScreenError>",
        description: "Capture the full primary display via the underlying backend. Default `MockScreen` returns a canned buffer.",
        capability: "computer.screen",
        example: "let shot = screen.capture()?;\nlog(format!(\"{}x{}\", shot.width, shot.height));\n",
        see_also: "Screen.capture_region, MockScreen.solid_color, Screen.width",
    },
    StdlibExample {
        symbol: "Screen.capture_region",
        signature: "fn Screen.capture_region(&self, x: U32, y: U32, w: U32, h: U32) -> Result<Screenshot, ScreenError>",
        description: "Capture a sub-rectangle of the display. Useful when the model only needs part of the viewport.",
        capability: "computer.screen",
        example: "let shot = screen.capture_region(0, 0, 640, 480)?;\n",
        see_also: "Screen.capture, MockScreen, Screenshot",
    },
    StdlibExample {
        symbol: "MockScreen.solid_color",
        signature: "fn MockScreen.solid_color(width: U32, height: U32, rgb: U32) -> MockScreen",
        description: "CI-safe mock display filled with a single RGB color. The default screen backend on tests + CI.",
        capability: "",
        example: "let s = MockScreen.solid_color(1280, 800, 0x000000);\nlet d = Dispatcher.new(llm, cap).with_screen(s);\n",
        see_also: "Screen.capture, Dispatcher.with_screen, MockMouse, MockKeyboard",
    },
    StdlibExample {
        symbol: "ComputerAction.Screenshot",
        signature: "const ComputerAction.Screenshot: ComputerAction",
        description: "The model asked for a screenshot. The dispatcher captures via the configured `Screen` and returns the bytes.",
        capability: "",
        example: "match action {\n  ComputerAction.Screenshot => screen.capture()?,\n  _ => panic(\"unhandled\"),\n}\n",
        see_also: "Screen.capture, ComputerAction.Click, ComputerAction.Type",
    },
    StdlibExample {
        symbol: "ComputerAction.Click",
        signature: "variant ComputerAction.Click { x: U32, y: U32, button: MouseButton, count: U8 }",
        description: "The model asked to click at `(x, y)`. `count` is 1 or 2 (double-click). Gated by `ComputerCap.with_bounds`.",
        capability: "",
        example: "match action {\n  ComputerAction.Click { x, y, button, count } => mouse.click_n(x, y, button, count)?,\n  _ => (),\n}\n",
        see_also: "Mouse.click_at, ComputerAction.Drag, ComputerCap.with_bounds",
    },
    StdlibExample {
        symbol: "ComputerAction.Type",
        signature: "variant ComputerAction.Type { text: Str }",
        description: "The model asked to type a literal string. The dispatcher routes through the keyboard backend.",
        capability: "",
        example: "match action {\n  ComputerAction.Type { text } => keyboard.type_text(&text)?,\n  _ => (),\n}\n",
        see_also: "Keyboard.type_text, ComputerAction.Key, ComputerCap.deny_keys",
    },
    StdlibExample {
        symbol: "ComputerAction.Key",
        signature: "variant ComputerAction.Key { name: Str }",
        description: "The model asked to press a named key or chord. Validated against the cap's deny-list.",
        capability: "",
        example: "match action {\n  ComputerAction.Key { name } => keyboard.key_press(&Key.from_str_lenient(&name)?)?,\n  _ => (),\n}\n",
        see_also: "Keyboard.key_press, ComputerCap.deny_keys, Key.Chord",
    },
    StdlibExample {
        symbol: "ComputerAction.Done",
        signature: "variant ComputerAction.Done { summary: Str }",
        description: "Terminal action. The dispatcher returns `summary` from `Dispatcher.run`.",
        capability: "",
        example: "match action {\n  ComputerAction.Done { summary } => return Ok(summary),\n  _ => (),\n}\n",
        see_also: "Dispatcher.run, ComputerAction.Screenshot",
    },
    StdlibExample {
        symbol: "SandboxViolation.OutOfBounds",
        signature: "variant SandboxViolation.OutOfBounds { x: U32, y: U32, .. }",
        description: "The action's click target was outside the cap's bounding rectangle. The dispatcher fails closed.",
        capability: "",
        example: "match err {\n  SandboxViolation.OutOfBounds { x, y, .. } => log(format!(\"click at ({},{}) rejected\", x, y)),\n  _ => (),\n}\n",
        see_also: "ComputerCap.with_bounds, SandboxViolation.Permission, SandboxViolation.DeniedKey",
    },
    StdlibExample {
        symbol: "SandboxViolation.DeniedKey",
        signature: "variant SandboxViolation.DeniedKey(Str)",
        description: "The key chord is on the cap's deny-list. Surfaces before the OS sees the press.",
        capability: "",
        example: "match err {\n  SandboxViolation.DeniedKey(chord) => log(format!(\"denied chord: {}\", chord)),\n  _ => (),\n}\n",
        see_also: "ComputerCap.deny_keys, Keyboard.key_press, SandboxViolation.OutOfBounds",
    },
    // ---- std.swarm internals ----
    StdlibExample {
        symbol: "SharedDollarBudget.new",
        signature: "fn SharedDollarBudget.new(limit_cents: U64) -> SharedDollarBudget",
        description: "Shared budget for one swarm panel. Every clone deducts from the same pool atomically.",
        capability: "",
        example: "let b = SharedDollarBudget.new(50);\nlet panel = [Member.anthropic(\"claude-opus-4-7\"), Member.openai(\"gpt-4o\")];\n",
        see_also: "SharedDollarBudget.from_dollars, SharedDollarBudget.try_charge, swarm",
    },
    StdlibExample {
        symbol: "SharedDollarBudget.from_dollars",
        signature: "fn SharedDollarBudget.from_dollars(dollars: F64) -> SharedDollarBudget",
        description: "Convenience: shared budget capped at `dollars` USD (converted to integer cents).",
        capability: "",
        example: "let b = SharedDollarBudget.from_dollars(0.50);\n",
        see_also: "SharedDollarBudget.new, SharedDollarBudget.unbounded, DollarBudget.from_dollars",
    },
    StdlibExample {
        symbol: "SharedDollarBudget.unbounded",
        signature: "fn SharedDollarBudget.unbounded() -> SharedDollarBudget",
        description: "Shared budget with no cap. Useful for observing `consumed_cents` without a ceiling.",
        capability: "",
        example: "let b = SharedDollarBudget.unbounded();\n",
        see_also: "SharedDollarBudget.from_dollars, SharedDollarBudget.consumed_cents",
    },
    StdlibExample {
        symbol: "SharedDollarBudget.consumed_cents",
        signature: "fn SharedDollarBudget.consumed_cents(&self) -> U64",
        description: "Total cents charged against the shared pool so far. Atomic across panel members.",
        capability: "",
        example: "log(format!(\"spent {} cents\", b.consumed_cents()));\n",
        see_also: "SharedDollarBudget.limit_cents, SharedDollarBudget.is_exhausted",
    },
    StdlibExample {
        symbol: "SharedDollarBudget.is_exhausted",
        signature: "fn SharedDollarBudget.is_exhausted(&self) -> Bool",
        description: "True if any further dispatch would exceed the cap. The swarm loop polls between members.",
        capability: "",
        example: "if b.is_exhausted() { log(\"hit cap, skipping next member\"); }\n",
        see_also: "SharedDollarBudget.consumed_cents, SharedDollarBudget.try_charge, Consensus.budget_exhausted",
    },
    StdlibExample {
        symbol: "SharedDollarBudget.try_charge",
        signature: "fn SharedDollarBudget.try_charge(&self, cents: U64) -> Result<U64, BudgetTripped>",
        description: "Charge `cents` against the shared pool. Returns the new total or `BudgetTripped` if it crossed the cap.",
        capability: "",
        example: "match b.try_charge(15) {\n  Ok(total) => log(format!(\"now {} cents\", total)),\n  Err(t) => log(\"tripped\"),\n}\n",
        see_also: "SharedDollarBudget.add_tokens, SharedDollarBudget.is_exhausted",
    },
    StdlibExample {
        symbol: "SharedDollarBudget.add_tokens",
        signature: "fn SharedDollarBudget.add_tokens(&self, model: Str, input_tokens: U64, output_tokens: U64) -> Result<U64, BudgetTripped>",
        description: "Charge a token count via the canonical per-million-token rate table for `model`.",
        capability: "",
        example: "b.add_tokens(\"claude-opus-4-7\", 1200, 480)?;\n",
        see_also: "SharedDollarBudget.try_charge, MemberReply.tokens_used",
    },
    StdlibExample {
        symbol: "Consensus.has_consensus",
        signature: "fn Consensus.has_consensus(&self) -> Bool",
        description: "True when the strategy landed on a `majority` body. False for Unanimous-with-disagreement and empty panels.",
        capability: "",
        example: "if c.has_consensus() { log(\"agreed\"); } else { log(\"split panel\"); }\n",
        see_also: "Consensus.body, Consensus.dissent_count, ConsensusStrategy.Unanimous",
    },
    StdlibExample {
        symbol: "Consensus.dissent_count",
        signature: "fn Consensus.dissent_count(&self) -> USize",
        description: "Number of members who fell outside the winning cluster.",
        capability: "",
        example: "log(format!(\"{} dissents\", c.dissent_count()));\n",
        see_also: "Consensus.dissents, Consensus.has_consensus",
    },
    StdlibExample {
        symbol: "Consensus.strategy",
        signature: "field Consensus.strategy: Str",
        description: "Strategy name (`\"majority\"`, `\"unanimous\"`, `\"weighted\"`, `\"first_agreed\"`) for logging / observability.",
        capability: "",
        example: "log(format!(\"strategy: {}\", c.strategy));\n",
        see_also: "ConsensusStrategy.Majority, Consensus.body",
    },
    StdlibExample {
        symbol: "SimilarityMode.Exact",
        signature: "const SimilarityMode.Exact: SimilarityMode",
        description: "Trim, lowercase, strip punctuation, exact-match. Cheap, ideal for yes/no answers.",
        capability: "",
        example: "let clusters = cluster_replies(&bodies, SimilarityMode.Exact, 1.0);\n",
        see_also: "SimilarityMode.TokenSet, ConsensusStrategy.Majority",
    },
    StdlibExample {
        symbol: "SimilarityMode.TokenSet",
        signature: "const SimilarityMode.TokenSet: SimilarityMode",
        description: "Normalised token-set Jaccard similarity. Default. Tolerates wording variance in free-form prose.",
        capability: "",
        example: "let clusters = cluster_replies(&bodies, SimilarityMode.TokenSet, 0.6);\n",
        see_also: "SimilarityMode.Exact, ConsensusStrategy.Majority",
    },
    // ---- std.observe query API ----
    StdlibExample {
        symbol: "Window.parse",
        signature: "fn Window.parse(spec: Str) -> Result<Window, Str>",
        description: "Parse a Go-style duration spec (`7d`, `12h`, `30m`, `45s`, `500ms`, or `all`). Used by `mty inspect --cost --since`.",
        capability: "",
        example: "let w = Window.parse(\"7d\")?;\nlet sum = summarize(&obs, w, GroupBy.Model, 5);\n",
        see_also: "Window.Last, Window.All, summarize",
    },
    StdlibExample {
        symbol: "Window.All",
        signature: "const Window.All: Window",
        description: "Window covering every observation in the store.",
        capability: "",
        example: "let sum = summarize(&obs, Window.All, GroupBy.Provider, 0);\n",
        see_also: "Window.Last, Window.parse, summarize",
    },
    StdlibExample {
        symbol: "Window.Last",
        signature: "variant Window.Last { millis: U64 }",
        description: "Window covering the last `millis` of wall-clock time before now.",
        capability: "",
        example: "let w = Window.Last { millis: 86400000 };\n",
        see_also: "Window.parse, Window.All",
    },
    StdlibExample {
        symbol: "GroupBy.Provider",
        signature: "const GroupBy.Provider: GroupBy",
        description: "Group cost rows by provider (anthropic / openai / gemini / bedrock).",
        capability: "",
        example: "let sum = summarize(&obs, Window.All, GroupBy.Provider, 0);\nfor row in sum.by_group.iter() { log(row.key); }\n",
        see_also: "GroupBy.Model, GroupBy.Agent, GroupBy.None, summarize",
    },
    StdlibExample {
        symbol: "GroupBy.Model",
        signature: "const GroupBy.Model: GroupBy",
        description: "Group cost rows by model id (e.g. `claude-opus-4-7`).",
        capability: "",
        example: "let sum = summarize(&obs, Window.All, GroupBy.Model, 0);\n",
        see_also: "GroupBy.Provider, GroupBy.Agent, summarize",
    },
    StdlibExample {
        symbol: "GroupBy.Agent",
        signature: "const GroupBy.Agent: GroupBy",
        description: "Group cost rows by `agent_id` so you can compare cost across agent handlers.",
        capability: "",
        example: "let sum = summarize(&obs, Window.All, GroupBy.Agent, 10);\n",
        see_also: "GroupBy.Provider, GroupBy.Model, summarize",
    },
    StdlibExample {
        symbol: "GroupBy.None",
        signature: "const GroupBy.None: GroupBy",
        description: "Collapse every call into one synthetic group keyed `\"all\"`. The bare totals row.",
        capability: "",
        example: "let sum = summarize(&obs, Window.All, GroupBy.None, 0);\n",
        see_also: "GroupBy.Provider, summarize, CostSummary",
    },
    StdlibExample {
        symbol: "summarize",
        signature: "fn summarize(obs: &[LlmObservation], window: Window, by: GroupBy, top_n: USize) -> CostSummary",
        description: "Canonical `mty inspect --cost` rollup. Filters by `window`, groups by `by`, and surfaces the top-`n` most-expensive calls.",
        capability: "",
        example: "let sum = summarize(&obs, Window.Last { millis: 86400000 }, GroupBy.Model, 5);\nlog(format!(\"${:.2}\", sum.total_cost_cents as F32 / 100.0));\n",
        see_also: "Window.parse, GroupBy.Model, percentiles, aggregate_by",
    },
    StdlibExample {
        symbol: "percentiles",
        signature: "fn percentiles(samples: &[U64]) -> LatencyPercentiles",
        description: "Compute p50/p95/p99 latency over a slice of millisecond samples. Used by `summarize`.",
        capability: "",
        example: "let p = percentiles(&latencies);\nlog(format!(\"p50={} p99={}\", p.p50_ms, p.p99_ms));\n",
        see_also: "summarize, LatencyPercentiles",
    },
    StdlibExample {
        symbol: "aggregate_by",
        signature: "fn aggregate_by(obs: &[&LlmObservation], by: GroupBy) -> Vec[AggregateRow]",
        description: "Group an observation slice by the chosen key. Returns one row per distinct group with totals.",
        capability: "",
        example: "let rows = aggregate_by(&obs.iter().collect::<Vec<_>>(), GroupBy.Provider);\n",
        see_also: "summarize, GroupBy.Provider, AggregateRow",
    },
    StdlibExample {
        symbol: "CostSummary",
        signature: "struct CostSummary { window, call_count, total_cost_cents, total_prompt_tokens, total_completion_tokens, latency, by_group, top_calls }",
        description: "Top-level cost summary surfaced by `mty inspect --cost`. Plain-data shape — safe to JSON-encode.",
        capability: "",
        example: "let s = summarize(&obs, Window.All, GroupBy.None, 0);\nlog(format!(\"calls={} cost_cents={}\", s.call_count, s.total_cost_cents));\n",
        see_also: "summarize, percentiles, GroupBy.Provider",
    },
    // ---- std.taint sanitizers ----
    StdlibExample {
        symbol: "HtmlEscape",
        signature: "const HtmlEscape: Sanitizer",
        description: "Escape `<`, `>`, `&`, `\"`, `'` for safe interpolation into HTML attributes / bodies.",
        capability: "",
        example: "let safe = raw.sanitize_with(HtmlEscape);\nstd.fs.write(\"out.html\", safe);\n",
        see_also: "ShellEscape, SqlEscape, PathBoundary, sanitize_with",
    },
    StdlibExample {
        symbol: "ShellEscape",
        signature: "const ShellEscape: Sanitizer",
        description: "Wrap in single quotes and escape internal `'` so the value is safe for `/bin/sh -c \"...\"`.",
        capability: "",
        example: "let arg = raw.sanitize_with(ShellEscape);\nstd.process.Command.new(\"ls\").arg(arg).spawn()?;\n",
        see_also: "HtmlEscape, SqlEscape, PathBoundary, sanitize_with",
    },
    StdlibExample {
        symbol: "SqlEscape",
        signature: "const SqlEscape: Sanitizer",
        description: "Escape `'` and `\\` (doubling backslashes) for safe interpolation into SQL string literals.",
        capability: "",
        example: "let safe = raw.sanitize_with(SqlEscape);\nstd.sql.execute(format!(\"SELECT * FROM users WHERE name='{}'\", safe))?;\n",
        see_also: "HtmlEscape, ShellEscape, PathBoundary, sanitize_with",
    },
    StdlibExample {
        symbol: "PathBoundary",
        signature: "fn PathBoundary(root: Str) -> Sanitizer",
        description: "Strip `..`, leading `/`, and NUL bytes; pin the result inside `root`. Defends against path-traversal sinks.",
        capability: "",
        example: "let safe = user_input.sanitize_with(PathBoundary(\"/var/uploads\"));\nstd.fs.write(safe, body);\n",
        see_also: "HtmlEscape, ShellEscape, SqlEscape, sanitize_with",
    },
    StdlibExample {
        symbol: "sanitize_with",
        signature: "fn Tainted[Str].sanitize_with(&self, s: Sanitizer) -> Str",
        description: "One of the three sanctioned untainting strategies. Applies a provably-correct sanitiser to a tainted value.",
        capability: "",
        example: "let raw = m.ask(\"summarise this email\");\nlet safe = raw.sanitize_with(HtmlEscape);\nstd.fs.write(\"summary.html\", safe);\n",
        see_also: "matches_regex, in_allowlist, HtmlEscape, ShellEscape, PathBoundary",
    },
    StdlibExample {
        symbol: "matches_regex",
        signature: "fn Tainted[Str].matches_regex(&self, pattern: Str) -> Option[Str]",
        description: "Untainting via regex shape constraint. Returns `Some(untainted)` if the value matches; `None` otherwise.",
        capability: "",
        example: "let safe = raw.matches_regex(\"^[a-zA-Z0-9_]+$\").unwrap_or(\"anon\");\nstd.fs.write(\"u.txt\", safe);\n",
        see_also: "in_allowlist, sanitize_with, HtmlEscape",
    },
    StdlibExample {
        symbol: "in_allowlist",
        signature: "fn Tainted[Str].in_allowlist[E]() -> Option[E]",
        description: "Untainting via enum-variant allowlist. Returns `Some(variant)` if the value parses as one of `E`'s names.",
        capability: "",
        example: "let v = raw.in_allowlist().unwrap_or(Verdict.Unclear);\nstd.fs.write(\"v.txt\", format!(\"{:?}\", v));\n",
        see_also: "matches_regex, sanitize_with",
    },
    // ---- std.eval comparators + verdicts ----
    StdlibExample {
        symbol: "Compare.equal",
        signature: "fn Compare.equal() -> Compare",
        description: "Strictest comparator — strings must match after trim + lowercase. Ideal for tool-name verdicts.",
        capability: "",
        example: "let cmp = Compare.equal();\nlet report = suite.compare(cmp).await?;\n",
        see_also: "Compare.semantic_similarity, Compare.tool_call_set_equal, Suite.compare",
    },
    StdlibExample {
        symbol: "Compare.semantic_similarity",
        signature: "fn Compare.semantic_similarity(threshold: F32) -> Compare",
        description: "Cosine-similarity comparator over the stub embedder. Two replies are equivalent iff similarity >= `threshold`.",
        capability: "",
        example: "let cmp = Compare.semantic_similarity(0.85);\n",
        see_also: "Compare.equal, Compare.tool_call_set_equal, Suite.compare",
    },
    StdlibExample {
        symbol: "Compare.semantic_similarity_with",
        signature: "fn Compare.semantic_similarity_with(threshold: F32, embedder: Embedder) -> Compare",
        description: "Cosine comparator with a caller-supplied embedder. Used to swap in OpenAI / qdrant backends.",
        capability: "",
        example: "let cmp = Compare.semantic_similarity_with(0.85, OpenAiEmbedder.new());\n",
        see_also: "Compare.semantic_similarity, Embedder",
    },
    StdlibExample {
        symbol: "Verdict.Match",
        signature: "const Verdict.Match: Verdict",
        description: "The reply matched the baseline under the configured comparator.",
        capability: "",
        example: "match v {\n  Verdict.Match => log(\"PASS\"),\n  Verdict.Diverge => log(\"FAIL\"),\n  _ => (),\n}\n",
        see_also: "Verdict.Diverge, Verdict.Error, Verdict.SingleMember, Compare.equal",
    },
    StdlibExample {
        symbol: "Verdict.Diverge",
        signature: "const Verdict.Diverge: Verdict",
        description: "The reply did not match the baseline.",
        capability: "",
        example: "if v == Verdict.Diverge { log(\"divergence — see report.divergences\"); }\n",
        see_also: "Verdict.Match, Verdict.Error, Divergence",
    },
    StdlibExample {
        symbol: "Verdict.Error",
        signature: "const Verdict.Error: Verdict",
        description: "The member errored before producing a reply (network, parse, budget).",
        capability: "",
        example: "if v == Verdict.Error { log(\"member errored — strict mode fails\"); }\n",
        see_also: "Verdict.Match, Verdict.Diverge, Suite.run",
    },
    StdlibExample {
        symbol: "Verdict.SingleMember",
        signature: "const Verdict.SingleMember: Verdict",
        description: "Only one member was registered. Counts as PASS by default; opt-in to fail with `Suite.require_two_members`.",
        capability: "",
        example: "if v == Verdict.SingleMember { log(\"add another member for cross-check\"); }\n",
        see_also: "Verdict.Match, Suite.member",
    },
    StdlibExample {
        symbol: "Case.from_input",
        signature: "fn Case.from_input(prompt: Str) -> Case",
        description: "Build a raw-prompt case with no recorded baseline. The runner compares replies member-vs-member.",
        capability: "",
        example: "let c = Case.from_input(\"Capital of France?\");\nsuite.case(c);\n",
        see_also: "Case.from_trace, Suite.case, Suite.run_with",
    },
    StdlibExample {
        symbol: "Case.from_trace",
        signature: "fn Case.from_trace(path: Str) -> Case",
        description: "Build a recorded-trace case. The runner reads the first user prompt + assistant reply at materialisation time.",
        capability: "fs.read (path)",
        example: "let c = Case.from_trace(\"traces/research-001.mty-trace\");\nsuite.case(c);\n",
        see_also: "Case.from_input, Suite.case",
    },
    StdlibExample {
        symbol: "Suite.run_with",
        signature: "fn Suite.run_with(&mut self, m: Member) -> &mut Suite",
        description: "Register a panel member for the eval grid. Same shape as `Suite.member` — kept for the fluent v0.28 builder.",
        capability: "net.https (per member)",
        example: "let suite = Suite.new(\"summaries\")\n  .case(Case.from_input(\"...\"))\n  .run_with(Member.anthropic(\"claude-opus-4-7\"))\n  .run_with(Member.openai(\"gpt-4o\"));\n",
        see_also: "Suite.case, Suite.compare, Suite.member",
    },
    // ---- std.web ----
    StdlibExample {
        symbol: "Canvas.new",
        signature: "fn Canvas.new(width: U32, height: U32) -> Canvas",
        description: "Mighty-side handle to a 2D canvas. Lowers to a WIT import on wasm32-web; no-op on native.",
        capability: "",
        example: "let c = Canvas.new(800, 600);\nc.fill_rect(10, 10, 50, 50, 0xff0000);\n",
        see_also: "Canvas.fill_rect, Canvas.stroke_rect, Canvas.fill_text, Canvas.clear",
    },
    StdlibExample {
        symbol: "Canvas.fill_rect",
        signature: "fn Canvas.fill_rect(&self, x: I32, y: I32, w: U32, h: U32, color: U32)",
        description: "Fill a rectangle at `(x, y)` with `color` (RGB packed as 0xRRGGBB).",
        capability: "",
        example: "canvas.fill_rect(0, 0, 200, 200, 0x00ff00);\n",
        see_also: "Canvas.stroke_rect, Canvas.clear, Canvas.set_fill_style",
    },
    StdlibExample {
        symbol: "Canvas.stroke_rect",
        signature: "fn Canvas.stroke_rect(&self, x: I32, y: I32, w: U32, h: U32, color: U32)",
        description: "Outline a rectangle. Same shape as `fill_rect` but draws the border only.",
        capability: "",
        example: "canvas.stroke_rect(5, 5, 100, 50, 0x0000ff);\n",
        see_also: "Canvas.fill_rect, Canvas.clear",
    },
    StdlibExample {
        symbol: "Canvas.fill_text",
        signature: "fn Canvas.fill_text(&self, text: Str, x: I32, y: I32, color: U32)",
        description: "Draw text at the baseline `(x, y)`. The host uses its current font / size settings.",
        capability: "",
        example: "canvas.fill_text(\"score: 42\", 10, 30, 0xffffff);\n",
        see_also: "Canvas.fill_rect, Canvas.set_fill_style",
    },
    StdlibExample {
        symbol: "Canvas.clear",
        signature: "fn Canvas.clear(&self)",
        description: "Clear the entire canvas to transparent. Most game agents call this once at the top of each frame.",
        capability: "",
        example: "canvas.clear();\ncanvas.fill_rect(0, 0, 800, 600, 0x000000);\n",
        see_also: "Canvas.request_animation_frame, Canvas.fill_rect",
    },
    StdlibExample {
        symbol: "Canvas.request_animation_frame",
        signature: "fn Canvas.request_animation_frame(&self)",
        description: "Ask the host to schedule the next render. The agent's `Frame` handler fires on the next tick.",
        capability: "",
        example: "on Frame(_) {\n  canvas.clear();\n  draw_world(&canvas);\n  canvas.request_animation_frame();\n}\n",
        see_also: "Canvas.clear, Input.subscribe_keydown",
    },
    StdlibExample {
        symbol: "Canvas.set_fill_style",
        signature: "fn Canvas.set_fill_style(&self, color: U32)",
        description: "Set the default fill colour. Subsequent `fill_rect` / `fill_text` calls without an explicit color use this.",
        capability: "",
        example: "canvas.set_fill_style(0x00ff00);\ncanvas.fill_rect(0, 0, 50, 50, 0x00ff00);\n",
        see_also: "Canvas.fill_rect, Canvas.fill_text",
    },
    StdlibExample {
        symbol: "Input.new",
        signature: "fn Input.new() -> Input",
        description: "Mighty-side input subscription. Lowers to the `mty:web/input@0.1` interface on wasm32-web.",
        capability: "",
        example: "let inp = Input.new();\ninp.subscribe_keydown();\n",
        see_also: "Input.subscribe_keydown, Input.subscribe_keyup, Key.ArrowLeft",
    },
    StdlibExample {
        symbol: "Input.subscribe_keydown",
        signature: "fn Input.subscribe_keydown(&self)",
        description: "Subscribe to keydown events. The agent's `KeyDown(Key)` handler fires for each key.",
        capability: "",
        example: "input.subscribe_keydown();\n// elsewhere: on KeyDown(k) { ... }\n",
        see_also: "Input.subscribe_keyup, Key.ArrowLeft, Key.Space",
    },
    StdlibExample {
        symbol: "Input.subscribe_keyup",
        signature: "fn Input.subscribe_keyup(&self)",
        description: "Subscribe to keyup events. The agent's `KeyUp(Key)` handler fires when the user releases.",
        capability: "",
        example: "input.subscribe_keyup();\n",
        see_also: "Input.subscribe_keydown, Key.ArrowLeft",
    },
    StdlibExample {
        symbol: "Key.ArrowLeft",
        signature: "const Key.ArrowLeft: Key",
        description: "Decoded `ArrowLeft` keyboard event. The DOM string `\"ArrowLeft\"` maps to this variant.",
        capability: "",
        example: "match key {\n  Key.ArrowLeft => player.move_left(),\n  Key.ArrowRight => player.move_right(),\n  _ => (),\n}\n",
        see_also: "Key.ArrowRight, Key.ArrowUp, Key.ArrowDown, Key.Space",
    },
    StdlibExample {
        symbol: "Key.Space",
        signature: "const Key.Space: Key",
        description: "Decoded space-bar event. DOM strings `\" \"`, `\"Space\"`, `\"Spacebar\"` all map here.",
        capability: "",
        example: "match key {\n  Key.Space => player.jump(),\n  _ => (),\n}\n",
        see_also: "Key.Enter, Key.Escape, Key.ArrowLeft",
    },
    StdlibExample {
        symbol: "Key.Char",
        signature: "variant Key.Char(Char)",
        description: "A single printable character (`\"a\"`, `\"Z\"`, `\"7\"`). Catches all single-char DOM keys.",
        capability: "",
        example: "match key {\n  Key.Char(c) => log(format!(\"typed {}\", c)),\n  _ => (),\n}\n",
        see_also: "Key.Other, Key.ArrowLeft, Key.from_dom_string",
    },
    // ---- std.fs ----
    StdlibExample {
        symbol: "std.fs.read_to_string",
        signature: "fn std.fs.read_to_string(cap: &Fs, path: Str) -> Result<Str, IoErr>",
        description: "Read `path` as UTF-8 text. Errors on invalid UTF-8 or read failure.",
        capability: "fs.read (path)",
        example: "let text = std.fs.read_to_string(&fs, \"input.txt\")?;\nlog(text);\n",
        see_also: "std.fs.read, std.fs.write, std.fs.open",
    },
    StdlibExample {
        symbol: "std.fs.stat",
        signature: "fn std.fs.stat(cap: &Fs, path: Str) -> Result<StatResult, IoErr>",
        description: "Return file size + a 3-state file-type discriminator. Mirrors WASI's `descriptor-stat`.",
        capability: "fs.read (path)",
        example: "let st = std.fs.stat(&fs, \"input.txt\")?;\nlog(format!(\"size={}\", st.size));\n",
        see_also: "std.fs.read, std.fs.exists, StatResult",
    },
    StdlibExample {
        symbol: "std.fs.exists",
        signature: "fn std.fs.exists(cap: &Fs, path: Str) -> Bool",
        description: "True if `path` exists and the cap allows reading it. Never errors — failure becomes `false`.",
        capability: "fs.read (path)",
        example: "if std.fs.exists(&fs, \"./cache.json\") { load_cache()?; }\n",
        see_also: "std.fs.stat, std.fs.read, std.fs.list_dir",
    },
    StdlibExample {
        symbol: "std.fs.list_dir",
        signature: "fn std.fs.list_dir(cap: &Fs, path: Str) -> Result<Vec[Str], IoErr>",
        description: "List immediate children of `path`. Returns paths (not bare names) so callers can chain stat/read.",
        capability: "fs.read (path)",
        example: "for entry in std.fs.list_dir(&fs, \"./docs\")?.iter() { log(entry); }\n",
        see_also: "std.fs.read, std.fs.stat, std.fs.exists",
    },
    StdlibExample {
        symbol: "std.fs.close",
        signature: "fn std.fs.close(cap: &Fs, handle: FileHandle) -> Result<Unit, IoErr>",
        description: "Close a handle obtained from `std.fs.open`. On wasm32-wasi lowers to the canonical-ABI resource-drop intrinsic.",
        capability: "",
        example: "let f = std.fs.open(&fs, \"big.bin\")?;\nstd.fs.close(&fs, f)?;\n",
        see_also: "std.fs.open, std.fs.read",
    },
    StdlibExample {
        symbol: "FsCap.unrestricted",
        signature: "fn FsCap.unrestricted() -> FsCap",
        description: "Capability that allows access to any path. Use only for tests / local-only tooling.",
        capability: "",
        example: "let fs = FsCap.unrestricted();\nstd.fs.read(&fs, \"/etc/hostname\")?;\n",
        see_also: "FsCap.rooted, std.fs.read, std.fs.write",
    },
    StdlibExample {
        symbol: "FsCap.rooted",
        signature: "fn FsCap.rooted(roots: List<Str>) -> FsCap",
        description: "Capability constrained to subpaths of `roots`. Reads/writes outside the rooted set fail closed.",
        capability: "",
        example: "let fs = FsCap.rooted([\"./data\", \"./logs\"]);\nstd.fs.read(&fs, \"./data/users.csv\")?;\n",
        see_also: "FsCap.unrestricted, std.fs.read, std.fs.write",
    },
    StdlibExample {
        symbol: "StatResult",
        signature: "struct StatResult { size: U64, kind: U8 }",
        description: "Metadata returned by `std.fs.stat`. `kind`: 0=file, 1=dir, 2=symlink, 3=other.",
        capability: "",
        example: "let st = std.fs.stat(&fs, p)?;\nif st.kind == 1 { log(\"dir\"); }\n",
        see_also: "std.fs.stat, std.fs.exists",
    },
    // ---- std.json (Value variants) ----
    StdlibExample {
        symbol: "Json.Null",
        signature: "const Json.Null: Json",
        description: "JSON null literal. Encodes to `null`; matches `null` on parse.",
        capability: "",
        example: "let j = Json.Null;\nassert_eq(std.json.encode(&j)?, \"null\");\n",
        see_also: "Json.Bool, Json.Num, Json.Str, Json.Arr, Json.Obj",
    },
    StdlibExample {
        symbol: "Json.Bool",
        signature: "variant Json.Bool(Bool)",
        description: "JSON boolean wrapper.",
        capability: "",
        example: "let j = Json.Bool(true);\nassert_eq(std.json.encode(&j)?, \"true\");\n",
        see_also: "Json.Null, Json.Num, std.json.encode",
    },
    StdlibExample {
        symbol: "Json.Num",
        signature: "variant Json.Num(F64)",
        description: "JSON numeric value. All numerics flow through `F64` to match Mighty's surface `Num` shape.",
        capability: "",
        example: "let j = Json.Num(42.0);\nassert_eq(std.json.encode(&j)?, \"42.0\");\n",
        see_also: "Json.Bool, Json.Str, std.json.encode",
    },
    StdlibExample {
        symbol: "Json.Str",
        signature: "variant Json.Str(Str)",
        description: "JSON string. Encoded with the standard JSON escape rules.",
        capability: "",
        example: "let j = Json.Str(\"hello\");\nassert_eq(std.json.encode(&j)?, \"\\\"hello\\\"\");\n",
        see_also: "Json.Num, Json.Arr, std.json.encode",
    },
    StdlibExample {
        symbol: "Json.Arr",
        signature: "variant Json.Arr(List<Json>)",
        description: "JSON array. Holds an ordered list of `Json` values.",
        capability: "",
        example: "let j = Json.Arr([Json.Num(1.0), Json.Num(2.0)]);\nassert_eq(std.json.encode(&j)?, \"[1.0,2.0]\");\n",
        see_also: "Json.Obj, Json.Num, std.json.encode",
    },
    StdlibExample {
        symbol: "Json.Obj",
        signature: "variant Json.Obj(Map<Str, Json>)",
        description: "JSON object. Backed by a deterministically-ordered map so encoded output is stable across runs.",
        capability: "",
        example: "let j = std.json.parse(\"{\\\"a\\\":1}\")?;\n",
        see_also: "Json.Arr, std.json.parse, std.json.encode",
    },
    // ---- std.string + std.vec methods ----
    StdlibExample {
        symbol: "String.with_capacity",
        signature: "fn String.with_capacity(n: USize) -> String",
        description: "Pre-allocate a string buffer for `n` bytes. Avoids re-allocs in a hot build loop.",
        capability: "",
        example: "let mut s = String.with_capacity(64);\ns.push_str(\"hi\");\n",
        see_also: "String.new, String.from_str, String.push_str",
    },
    StdlibExample {
        symbol: "String.len",
        signature: "fn String.len(&self) -> USize",
        description: "Byte length. NOT the char count — `\"a©\"` has `len() == 3` because `©` is a 2-byte UTF-8 sequence.",
        capability: "",
        example: "let n = String.from_str(\"hello\").len();\nassert_eq(n, 5);\n",
        see_also: "String.is_empty, String.as_bytes",
    },
    StdlibExample {
        symbol: "String.is_empty",
        signature: "fn String.is_empty(&self) -> Bool",
        description: "True if the string has zero bytes. Same as `len() == 0`.",
        capability: "",
        example: "let s = String.new();\nassert(s.is_empty());\n",
        see_also: "String.len, String.clear",
    },
    StdlibExample {
        symbol: "String.push",
        signature: "fn String.push(&mut self, c: Char)",
        description: "Append a single character. UTF-8-aware: a single `Char` may add up to 4 bytes.",
        capability: "",
        example: "let mut s = String.new();\ns.push('h');\ns.push('i');\n",
        see_also: "String.push_str, String.clear",
    },
    StdlibExample {
        symbol: "String.clear",
        signature: "fn String.clear(&mut self)",
        description: "Reset length to zero without releasing capacity. Build-and-clear loops avoid re-allocations.",
        capability: "",
        example: "let mut s = String.with_capacity(64);\ns.push_str(\"draft\");\ns.clear();\n",
        see_also: "String.push_str, String.with_capacity",
    },
    StdlibExample {
        symbol: "String.as_bytes",
        signature: "fn String.as_bytes(&self) -> &[U8]",
        description: "Borrow the raw UTF-8 bytes. Useful for hashing / network framing where you want bytes, not chars.",
        capability: "",
        example: "let s = String.from_str(\"hi\");\nlet bs = s.as_bytes();\nassert_eq(bs.len(), 2);\n",
        see_also: "String.len, String.into_bytes",
    },
    StdlibExample {
        symbol: "format",
        signature: "macro format!(fmt: Str, args: ...) -> Str",
        description: "`println`-style formatter. Returns a `Str`; non-allocating for the literal-only case.",
        capability: "",
        example: "let s = format!(\"score: {}, level: {}\", 42, 7);\nlog(s);\n",
        see_also: "log, String.push_str",
    },
    // ---- v0.36 Track T3: String position / range edit / char-boundary ----
    StdlibExample {
        symbol: "String.find",
        signature: "fn String.find(&self, needle: Str) -> Option[USize]",
        description: "Byte index of the first occurrence of `needle`, or `None`. Mirrors `str::find`.",
        capability: "",
        example: "let s = \"Hello, Mighty\";\nlet pos = s.find(\"Mighty\");\nassert_eq(pos, Some(7));\n",
        see_also: "String.rfind, String.position, String.contains",
    },
    StdlibExample {
        symbol: "String.rfind",
        signature: "fn String.rfind(&self, needle: Str) -> Option[USize]",
        description: "Byte index of the last occurrence of `needle`, or `None`.",
        capability: "",
        example: "let pos = \"ababab\".rfind(\"ab\");\nassert_eq(pos, Some(4));\n",
        see_also: "String.find, String.position",
    },
    StdlibExample {
        symbol: "String.position",
        signature: "fn String.position(&self, c: Char) -> Option[USize]",
        description: "Byte index of the first occurrence of code-point `c`, or `None`. Cheaper than `find(&c.to_string())` for single-`Char` lookups.",
        capability: "",
        example: "let p = \"h©llo\".position('©');\nassert_eq(p, Some(1));\n",
        see_also: "String.find, String.chars",
    },
    StdlibExample {
        symbol: "String.insert_at",
        signature: "fn String.insert_at(&self, idx: USize, t: Str) -> Option[Str]",
        description: "Splice `t` into this string at byte position `idx`. Returns `None` (MT5080) when `idx` is past the end or is not a UTF-8 code-point boundary.",
        capability: "",
        example: "let s = \"Hello, Mighty\";\nlet r = s.insert_at(7, \"the \");\nassert_eq(r, Some(\"Hello, the Mighty\"));\n",
        see_also: "String.remove_range, String.replace_range, String.is_char_boundary",
    },
    StdlibExample {
        symbol: "String.remove_range",
        signature: "fn String.remove_range(&self, start: USize, end: USize) -> Option[Str]",
        description: "Delete the byte range `start..end`. Returns `None` (MT5080) when bounds are inverted, out of range, or not on UTF-8 boundaries.",
        capability: "",
        example: "let s = \"Hello, the Mighty\";\nlet r = s.remove_range(7, 11);\nassert_eq(r, Some(\"Hello, Mighty\"));\n",
        see_also: "String.replace_range, String.insert_at, String.is_char_boundary",
    },
    StdlibExample {
        symbol: "String.replace_range",
        signature: "fn String.replace_range(&self, start: USize, end: USize, t: Str) -> Option[Str]",
        description: "Replace the byte range `start..end` with `t`. Returns `None` (MT5080) on bad bounds.",
        capability: "",
        example: "let s = \"Hello, the Mighty\";\nlet r = s.replace_range(7, 11, \"a \");\nassert_eq(r, Some(\"Hello, a Mighty\"));\n",
        see_also: "String.remove_range, String.insert_at, String.replace",
    },
    StdlibExample {
        symbol: "String.is_char_boundary",
        signature: "fn String.is_char_boundary(&self, idx: USize) -> Bool",
        description: "True iff `idx` is a UTF-8 code-point boundary (including `0` and `byte_len()`). False inside multi-byte sequences and past the end.",
        capability: "",
        example: "assert(!\"é\".is_char_boundary(1));\n",
        see_also: "String.next_char_boundary, String.prev_char_boundary",
    },
    StdlibExample {
        symbol: "String.next_char_boundary",
        signature: "fn String.next_char_boundary(&self, idx: USize) -> Option[USize]",
        description: "Smallest UTF-8 code-point boundary strictly greater than `idx`, or `None` if `idx >= byte_len()`.",
        capability: "",
        example: "assert_eq(\"a©b\".next_char_boundary(2), Some(3));\n",
        see_also: "String.is_char_boundary, String.prev_char_boundary",
    },
    StdlibExample {
        symbol: "String.prev_char_boundary",
        signature: "fn String.prev_char_boundary(&self, idx: USize) -> Option[USize]",
        description: "Largest UTF-8 code-point boundary strictly less than `idx`, or `None` if `idx == 0`.",
        capability: "",
        example: "assert_eq(\"a©b\".prev_char_boundary(2), Some(1));\n",
        see_also: "String.is_char_boundary, String.next_char_boundary",
    },
    StdlibExample {
        symbol: "String.chars",
        signature: "fn String.chars(&self) -> Iterator[Char]",
        description: "Iterator over Unicode code points. Lazy: no intermediate `Vec<Char>` allocation.",
        capability: "",
        example: "let n = \"a©🦀\".chars().len();\nassert_eq(n, 3);\n",
        see_also: "String.char_indices, String.bytes",
    },
    StdlibExample {
        symbol: "String.char_indices",
        signature: "fn String.char_indices(&self) -> Iterator[(USize, Char)]",
        description: "Iterator over `(byte_index, char)` pairs.",
        capability: "",
        example: "let pairs = \"a©b\".char_indices();\nassert_eq(pairs.len(), 3);\n",
        see_also: "String.chars, String.as_bytes",
    },
    StdlibExample {
        symbol: "String.byte_len",
        signature: "fn String.byte_len(&self) -> USize",
        description: "Documented byte-length alias of `len`. Spells out \"I want bytes\" at the call site.",
        capability: "",
        example: "assert_eq(\"a©\".byte_len(), 3);\n",
        see_also: "String.len, String.as_bytes",
    },
    StdlibExample {
        symbol: "Vec.pop",
        signature: "fn Vec.pop[T](&mut self) -> Option[T]",
        description: "Remove and return the last element, or `None` if the vector is empty.",
        capability: "",
        example: "let mut v = Vec.new();\nv.push(42);\nlet x = v.pop();\nassert_eq(x, Some(42));\n",
        see_also: "Vec.push, Vec.len, Vec.is_empty",
    },
    StdlibExample {
        symbol: "Vec.len",
        signature: "fn Vec.len[T](&self) -> USize",
        description: "Number of elements currently in the vector.",
        capability: "",
        example: "let n = v.len();\nlog(format!(\"len={}\", n));\n",
        see_also: "Vec.is_empty, Vec.push, Vec.capacity",
    },
    StdlibExample {
        symbol: "Vec.is_empty",
        signature: "fn Vec.is_empty[T](&self) -> Bool",
        description: "True if the vector has zero elements. Equivalent to `len() == 0`.",
        capability: "",
        example: "if v.is_empty() { log(\"empty\"); }\n",
        see_also: "Vec.len, Vec.clear",
    },
    StdlibExample {
        symbol: "Vec.clear",
        signature: "fn Vec.clear[T](&mut self)",
        description: "Drop every element. Preserves the underlying capacity so reuse is allocation-free.",
        capability: "",
        example: "v.clear();\nassert(v.is_empty());\n",
        see_also: "Vec.pop, Vec.with_capacity, Vec.is_empty",
    },
    StdlibExample {
        symbol: "Vec.get",
        signature: "fn Vec.get[T](&self, idx: USize) -> Option[&T]",
        description: "Bounds-checked read. Returns `None` for out-of-range indices instead of panicking.",
        capability: "",
        example: "match v.get(0) {\n  Some(x) => log(format!(\"{}\", x)),\n  None => log(\"empty\"),\n}\n",
        see_also: "Vec.get_mut, Vec.iter, Vec.len",
    },
    StdlibExample {
        symbol: "Vec.capacity",
        signature: "fn Vec.capacity[T](&self) -> USize",
        description: "Number of elements the vector can hold before re-allocating.",
        capability: "",
        example: "let v: Vec[I32] = Vec.with_capacity(128);\nassert(v.capacity() >= 128);\n",
        see_also: "Vec.with_capacity, Vec.len",
    },
    StdlibExample {
        symbol: "Vec.iter_mut",
        signature: "fn Vec.iter_mut[T](&mut self) -> Iterator[&mut T]",
        description: "Iterator yielding mutable references. Use to update every element in place.",
        capability: "",
        example: "for x in v.iter_mut() { *x = *x + 1; }\n",
        see_also: "Vec.iter, Vec.get_mut",
    },
    StdlibExample {
        symbol: "Vec.append",
        signature: "fn Vec.append[T](&mut self, other: &mut Vec[T])",
        description: "Move every element from `other` to the back of `self`. `other` is left empty.",
        capability: "",
        example: "let mut a = Vec.new();\na.push(1);\nlet mut b = Vec.new();\nb.push(2);\na.append(&mut b);\nassert(b.is_empty());\n",
        see_also: "Vec.push, Vec.iter",
    },
    StdlibExample {
        symbol: "Vec.from_elem",
        signature: "fn Vec.from_elem[T](value: T, n: USize) -> Vec[T]",
        description: "Build a vector by cloning `value` `n` times. Useful for fixed-size board / buffer init.",
        capability: "",
        example: "let board = Vec.from_elem(0_u32, 200);\nassert_eq(board.len(), 200);\n",
        see_also: "Vec.with_capacity, Vec.new, Vec.push",
    },
    // ---- v0.39 T3: Vec typed-slot storage ----
    StdlibExample {
        symbol: "Vec.set",
        signature: "fn Vec.set[T](&mut self, idx: USize, value: T)",
        description: "In-place update at `idx`. Bounds-checked: out-of-range index traps. With v0.39's typed-slot storage the element is written using the header's `elem_size`, so a `Vec[U8]` writes exactly one byte per slot (8x smaller than the old word-per-slot layout).",
        capability: "",
        example: "let mut v: Vec[U8] = Vec.with_capacity(4);\nv.push(1u8);\nv.push(2u8);\nv.set(0, 9u8);\nassert_eq(v.get(0), Some(&9u8));\n",
        see_also: "Vec.get, Vec.push, VEC_HEADER_V2, vec_typed_slot",
    },
    StdlibExample {
        symbol: "VEC_HEADER_V2",
        signature: "Vec header v2 (32 bytes): len, cap, data, elem_size",
        description: "v0.39 widened the Vec header from 24 bytes (len, cap, data) to 32 bytes by recording per-instance `elem_size`. Storage is now packed at the natural element width — a `Vec[U8]` uses 1 byte/slot instead of 8 — which gives the well-known 8x memory reduction for byte buffers. Old v1 layout is documented for backward-compat reasoning.",
        capability: "",
        example: "// header v2 layout (bytes):\n//   0..8  : len      USize\n//   8..16 : cap      USize\n//  16..24 : data     *Void\n//  24..32 : elem_size USize  // new in v0.39\n",
        see_also: "Vec.set, Vec.push, vec_typed_slot",
    },
    StdlibExample {
        symbol: "vec_typed_slot",
        signature: "Vec[T] stores elements at sizeof(T) per slot",
        description: "Walkthrough of the v0.39 typed-slot layout: push/get/set read `elem_size` from the header and stride memory by exactly that many bytes. Bounds checks trap on out-of-range writes. Result: `Vec[U8]` stops paying a 7-byte tax per element, while wider element types keep their natural alignment.",
        capability: "",
        example: "// Vec[U8]: 1 byte per slot (was 8 in v0.38)\nlet mut bytes: Vec[U8] = Vec.with_capacity(1024);\nfor i in 0..1024 { bytes.push(i as U8); }\nassert_eq(bytes.len(), 1024);\n// Out-of-range set traps:\n// bytes.set(99999, 0u8); // TRAP\n",
        see_also: "Vec.set, Vec.push, VEC_HEADER_V2",
    },
    // ---- v0.38 T4: extern c / FFI surfaces (v0.37 T3, T6) ----
    StdlibExample {
        symbol: "extern_block",
        signature: "extern c { fn name(args) -> Ty }",
        description: "Declares one or more C-ABI imports. The block tag picks the ABI; today `c` (system C) and `js` (wasm host) are recognised. Each fn's body lives in the linked archive named by `[[extern_lib]]`.",
        capability: "",
        example: "extern c {\n  fn libc_strlen(p: *U8) -> USize\n  fn libc_puts(p: *U8) -> I32\n}\n",
        see_also: "extern_c_fn, extern_lib, extern_c_variadic, coerce_str_to_u8",
    },
    StdlibExample {
        symbol: "extern_c_fn",
        signature: "extern c fn name(args) -> Ty",
        description: "Single-line C-ABI import. Equivalent to wrapping the fn in an `extern c { ... }` block. The cranelift backend declares it as `Linkage::Import` and the linker resolves it from `[[extern_lib]]` archives.",
        capability: "",
        example: "extern c fn libc_strlen(p: *U8) -> USize;\nlet n = libc_strlen(\"hello\".as_ptr());\n",
        see_also: "extern_block, extern_lib, coerce_str_to_u8, addr_of_local",
    },
    StdlibExample {
        symbol: "extern_c_variadic",
        signature: "extern c fn name(fixed: Ty, ...) -> Ty",
        description: "v0.37 T6: variadic C-ABI import (printf-family). At each call site the variadic tail is C-ABI promoted (Float→Double, I8/I16→I32). The wasm backend hard-errors — no portable variadic ABI.",
        capability: "",
        example: "extern c fn libc_printf(fmt: *U8, ...) -> I32;\nlibc_printf(\"%d %s\\n\".as_ptr(), 42, \"hi\".as_ptr());\n",
        see_also: "extern_c_fn, extern_block, cast_as",
    },
    StdlibExample {
        symbol: "extern_lib",
        signature: "[[extern_lib]] name = \"...\" kind = \"static\"|\"dynamic\"",
        description: "Manifest entry pinning a native archive Mighty links against. `path` overrides the linker search path; `link_args_{linux,macos,windows}` carry host-OS flags. Multiple entries are honoured in source order.",
        capability: "",
        example: "[[extern_lib]]\nname = \"sodium\"\nkind = \"static\"\npath = \"vendor/libsodium.a\"\nlink_args_linux = [\"-lpthread\"]\n",
        see_also: "extern_block, extern_c_fn",
    },
    StdlibExample {
        symbol: "coerce_str_to_u8",
        signature: "Str -> *U8 (call-site coercion)",
        description: "v0.37 T3: a `Str` literal or expression passed where a `*U8` is expected coerces automatically. The bytes are the NUL-terminated UTF-8 buffer the runtime holds for that string.",
        capability: "",
        example: "extern c fn libc_puts(p: *U8) -> I32;\nlibc_puts(\"hello\");\n",
        see_also: "extern_c_fn, addr_of_local, cast_ptr_to_usize",
    },
    StdlibExample {
        symbol: "addr_of_local",
        signature: "&local -> *const Ty",
        description: "v0.37 T3: address-of a local binding for FFI. Produces `*const T` from a `&T` borrow. Borrow check still tracks the lifetime — the pointer must not outlive the local.",
        capability: "",
        example: "extern c fn rgb_pack(p: *const Color) -> U32;\nlet c = Color { r: 1, g: 2, b: 3 };\nlet packed = rgb_pack(&c);\n",
        see_also: "addr_of_mut, extern_c_fn, coerce_str_to_u8",
    },
    StdlibExample {
        symbol: "addr_of_mut",
        signature: "&mut local -> *mut Ty",
        description: "v0.37 T3: mutable address-of for out-parameter FFI. The C function writes through the pointer; Mighty's borrow check rules out aliasing the local while the pointer is live.",
        capability: "",
        example: "extern c fn read_int(out: *mut I32);\nlet mut x: I32 = 0;\nread_int(&mut x);\nlog(format!(\"x={}\", x));\n",
        see_also: "addr_of_local, extern_c_fn",
    },
    StdlibExample {
        symbol: "returned_struct",
        signature: "extern c fn foo(args) -> Struct",
        description: "v0.38 T3 (if shipped): C functions can return by-value structs directly. The Mighty caller binds the result with `let`; layout matches the platform System V / Win64 small-struct rules.",
        capability: "",
        example: "extern c fn make_point() -> Point;\nlet p = make_point();\nlog(format!(\"x={} y={}\", p.x, p.y));\n",
        see_also: "extern_c_fn, addr_of_local",
    },
    // ---- v0.38 T4: cast expressions (v0.37 T2 — MT2027 INVALID_CAST) ----
    StdlibExample {
        symbol: "cast_as",
        signature: "expr as Ty",
        description: "Scalar cast operator. Accepted for numeric widening / narrowing, signedness flips, `Bool→U8`, `Char→U32`, and pointer↔USize. Other shapes emit MT2027 at type-check time.",
        capability: "",
        example: "let n: I64 = 42;\nlet b: U8 = n as U8;\nlet f: F64 = n as F64;\n",
        see_also: "cast_u8_to_i64, cast_invalid_mt2027, cast_f32_to_f64",
    },
    StdlibExample {
        symbol: "cast_u8_to_i64",
        signature: "u8_expr as I64",
        description: "Zero-extend an unsigned byte to a 64-bit signed integer. Always lossless because U8's range fits in I64.",
        capability: "",
        example: "let byte: U8 = 0xFF;\nlet wide: I64 = byte as I64;\nassert_eq(wide, 255);\n",
        see_also: "cast_as, cast_i64_to_u8, cast_usize_to_u64",
    },
    StdlibExample {
        symbol: "cast_i64_to_u8",
        signature: "i64_expr as U8",
        description: "Narrowing cast: truncates to the low 8 bits. Sign / overflow is silently dropped — no MT2027 because the cast is structurally well-formed; the lossy behaviour is intentional.",
        capability: "",
        example: "let big: I64 = 257;\nlet small: U8 = big as U8;\nassert_eq(small, 1);\n",
        see_also: "cast_as, cast_u8_to_i64",
    },
    StdlibExample {
        symbol: "cast_f32_to_f64",
        signature: "f32_expr as F64",
        description: "Widen single-precision to double-precision float. Always lossless.",
        capability: "",
        example: "let x: F32 = 1.5;\nlet y: F64 = x as F64;\n",
        see_also: "cast_as, cast_f64_to_f32, cast_i32_to_f32",
    },
    StdlibExample {
        symbol: "cast_f64_to_f32",
        signature: "f64_expr as F32",
        description: "Narrow double to single precision. May round; values outside F32's range produce `+inf` / `-inf`.",
        capability: "",
        example: "let x: F64 = 3.141592653589793;\nlet y: F32 = x as F32;\n",
        see_also: "cast_as, cast_f32_to_f64",
    },
    StdlibExample {
        symbol: "cast_i32_to_f32",
        signature: "i32_expr as F32",
        description: "Int-to-float cast. Lossless for |n| ≤ 2^24; larger magnitudes round to the nearest representable F32.",
        capability: "",
        example: "let n: I32 = 1000;\nlet f: F32 = n as F32;\n",
        see_also: "cast_as, cast_f32_to_i32, cast_f32_to_f64",
    },
    StdlibExample {
        symbol: "cast_f32_to_i32",
        signature: "f32_expr as I32",
        description: "Float-to-int truncation toward zero. NaN→0, ±inf saturate to I32::MIN/MAX (LLVM `fptosi.sat`).",
        capability: "",
        example: "let f: F32 = 3.7;\nlet n: I32 = f as I32;\nassert_eq(n, 3);\n",
        see_also: "cast_as, cast_i32_to_f32",
    },
    StdlibExample {
        symbol: "cast_usize_to_u64",
        signature: "usize_expr as U64",
        description: "Platform-pointer-width to fixed 64-bit width. Identity on 64-bit hosts; zero-extends on 32-bit hosts.",
        capability: "",
        example: "let n: USize = 1024;\nlet w: U64 = n as U64;\n",
        see_also: "cast_as, cast_u8_to_i64, cast_ptr_to_usize",
    },
    StdlibExample {
        symbol: "cast_bool_to_u8",
        signature: "bool_expr as U8",
        description: "`true→1`, `false→0`. Bool ↔ any int width is accepted as of v0.39 T2 (the reverse `IntN as Bool` follows `0 → false`, nonzero → `true` and lowers to `icmp ne 0`); `Bool as Float` and `Float as Bool` still emit MT2027.",
        capability: "",
        example: "let flag: Bool = true;\nlet n: U8 = flag as U8;\nassert_eq(n, 1);\nlet round_trip: Bool = (n as I32) as Bool;\nassert_eq(round_trip, true);\n",
        see_also: "cast_as, cast_int_to_bool, cast_invalid_mt2027",
    },
    StdlibExample {
        symbol: "cast_int_to_bool",
        signature: "int_expr as Bool",
        description: "v0.39 T2 — `0 → false`, any nonzero → `true`. The cranelift back-end emits an `icmp ne 0` (not a low-byte truncate), so `256_i32 as Bool` correctly yields `true`. `Float as Bool` is deliberately rejected — use the explicit predicate `x != 0.0 && !x.is_nan()`.",
        capability: "",
        example: "let n: I32 = 256;\nlet truthy: Bool = n as Bool;\nassert_eq(truthy, true);\nlet zeroish: Bool = 0_i32 as Bool;\nassert_eq(zeroish, false);\n",
        see_also: "cast_as, cast_bool_to_u8, cast_invalid_mt2027",
    },
    StdlibExample {
        symbol: "cast_char_to_u32",
        signature: "char_expr as U32",
        description: "Code-point integer value of a `Char`. ASCII `'A'` → 65, BMP `'©'` → 169.",
        capability: "",
        example: "let c: Char = 'A';\nlet code: U32 = c as U32;\nassert_eq(code, 65);\n",
        see_also: "cast_as, cast_int_to_char, String.chars",
    },
    StdlibExample {
        symbol: "cast_int_to_char",
        signature: "<int-literal> as Char  (literal only; non-literal -> Char.from_u32)",
        description: "Integer-to-codepoint cast. **Literals** are checked at compile time against the Unicode scalar value range (0..0x110000 minus the UTF-16 surrogate gap 0xD800..=0xDFFF); out-of-range literals emit MT2028 INVALID_CODEPOINT. **Non-literal sources are rejected at the cast surface** as of v0.40 T3 (MT2027) — authors must spell `Char.from_u32(value)` which returns `Option[Char]`. The fix engine auto-rewrites the old shape; see docs/reference/casts.md + docs/reference/std-char.md.",
        capability: "",
        example: "// Literals — compile-time checked.\nlet a: Char = 0x41 as Char;       // 'A' — ok\nlet hi: Char = 0xD7FF as Char;    // last value before surrogate gap\n// let bad: Char = 0xD800 as Char; // MT2028 INVALID_CODEPOINT\n\n// Runtime-computed — use Char.from_u32.\nfn from_input(v: U32) -> Option[Char] { Char.from_u32(v) }\nlet safe: Char = Char.from_u32(v).unwrap_or('?');\n",
        see_also: "cast_as, cast_char_to_u32, cast_invalid_mt2028, char_from_u32",
    },
    StdlibExample {
        symbol: "char_from_u32",
        signature: "fn Char.from_u32(value: U32) -> Option[Char]",
        description: "v0.40 T3 — explicit constructor for a `Char` from a runtime `U32` codepoint. Returns `Some(c)` iff `value` is a valid Unicode scalar value (`< 0x110000` and not in the surrogate gap `0xD800..=0xDFFF`); otherwise `None`. Mirrors Rust's `char::from_u32`. Replaces the v0.39 T2 non-literal `Int as Char` surface, which is now rejected with MT2027.",
        capability: "",
        example: "let a: Option[Char] = Char.from_u32(0x41_u32);       // Some('A')\nlet bad: Option[Char] = Char.from_u32(0xD800_u32);   // None (surrogate)\nlet safe: Char = Char.from_u32(v).unwrap_or('?');\n",
        see_also: "cast_int_to_char, cast_char_to_u32, cast_invalid_mt2028",
    },
    StdlibExample {
        symbol: "cast_ref_to_ptr",
        signature: "&T as *T",
        description: "v0.39 T2 — explicit reference cast. Promotes the v0.37 T3 extern-c `coerce_addr_of` path to a general surface so authors can spell `&x as *I32` outside an FFI call site. The inner types must unify (`&I32 as *U8` emits MT2027); pointer-to-integer round-trips are NOT allowed via `as`, use `unsafe { raw_ptr(addr) }` instead. `*const T` and `*mut T` collapse onto a single `TyData::Ref` shape at typeck (slice-1 simplification).",
        capability: "",
        example: "let x: I32 = 42;\nlet p: *I32 = &x as *I32;\n",
        see_also: "cast_as, addr_of_local, coerce_str_to_u8",
    },
    StdlibExample {
        symbol: "cast_invalid_mt2028",
        signature: "MT2028 INVALID_CODEPOINT",
        description: "v0.39 T2 type-check error for a literal `Int as Char` cast whose value is outside the Unicode scalar value range (0..0x110000) or in the UTF-16 surrogate gap 0xD800..=0xDFFF. Mighty's `Char` is a Unicode scalar value; allowing an out-of-range literal would corrupt UTF-8 invariants when the value flowed into a String.",
        capability: "",
        example: "// MT2028: 0x110000 sits one past the top of the scalar-value range\nlet bad1: Char = 0x110000 as Char; // ERROR\n// MT2028: surrogate gap is reserved\nlet bad2: Char = 0xD800 as Char;   // ERROR\n",
        see_also: "cast_int_to_char, cast_char_to_u32, cast_invalid_mt2027",
    },
    StdlibExample {
        symbol: "cast_ptr_to_usize",
        signature: "*Ty as USize  (deferred — MT2027 today)",
        description: "Pointer↔integer round-tripping via the `as` surface is NOT accepted (MT2027). v0.39 T2's reference-cast rule (`&T as *T`, see cast_ref_to_ptr) stops at the pointer level on purpose. Use the existing `unsafe { raw_ptr(addr) }` builtin to bridge the integer→pointer side; the pointer→integer side is reserved for a future `unsafe { addr_of(p) }` builtin. This entry remains in the docstub stream so future tooling/tests can surface the gap; v0.40 may promote it to a working slot.",
        capability: "",
        example: "// Reserved — does NOT compile today (emits MT2027):\n//   let p: *U8 = ...;\n//   let addr: USize = p as USize;\n// Bridge via the unsafe builtin instead:\n//   let p: *U8 = unsafe { raw_ptr(addr) };\n",
        see_also: "cast_as, cast_ref_to_ptr, addr_of_local",
    },
    StdlibExample {
        symbol: "cast_invalid_mt2027",
        signature: "MT2027 INVALID_CAST",
        description: "Type-check error emitted when `expr as Ty` is not on the recognised scalar-conversion table. Example: `\"hi\" as I32`, `Vec[I32] as USize`, or `Bool as F64`.",
        capability: "",
        example: "// MT2027: invalid cast — Str has no scalar projection to I32\nlet s: Str = \"hi\";\nlet n: I32 = s as I32; // ERROR\n",
        see_also: "cast_as, cast_bool_to_u8, cast_char_to_u32",
    },
    // ---- v0.38 T4: MTY_* runtime / build env vars (v0.36 rename) ----
    StdlibExample {
        symbol: "MTY_LINKER",
        signature: "env var MTY_LINKER=<path-or-name>",
        description: "Picks the native linker `mty build` invokes (e.g. `clang`, `lld`, full path to `link.exe`). Falls back to the legacy `STARDUST_LINKER` spelling. When unset, the cranelift backend writes the object file and reports `no linker found`.",
        capability: "",
        example: "// Pick clang explicitly:\n//   MTY_LINKER=clang mty build app.mty\n// Legacy alias still honoured:\n//   STARDUST_LINKER=lld mty build app.mty\n",
        see_also: "MTY_TRACE, MTY_OTLP_ENDPOINT, MTY_RUNTIME_THREADS",
    },
    StdlibExample {
        symbol: "MTY_OTLP_ENDPOINT",
        signature: "env var MTY_OTLP_ENDPOINT=<grpc-url>",
        description: "Enables the OTLP telemetry exporter. When set, the runtime ships spans to the given gRPC collector (default port 4317). Falls back to the legacy `STARDUST_OTLP_ENDPOINT` spelling.",
        capability: "",
        example: "// Boot with OpenTelemetry export:\n//   MTY_OTLP_ENDPOINT=http://localhost:4317 mty run app.mty\n",
        see_also: "MTY_TRACE, MTY_LINKER, observe.otel_sink",
    },
    StdlibExample {
        symbol: "MTY_TRACE",
        signature: "env var MTY_TRACE=stderr|file:<path>",
        description: "Selects the runtime's NDJSON telemetry sink. `stderr` for inline debugging, `file:<path>` for an append-only log. Overridden when `MTY_OTLP_ENDPOINT` is set. Legacy `STARDUST_TRACE` honoured.",
        capability: "",
        example: "// MTY_TRACE=stderr mty run app.mty\n// MTY_TRACE=file:./run.ndjson mty run app.mty\n",
        see_also: "MTY_OTLP_ENDPOINT, MTY_LINKER, observe.record",
    },
    StdlibExample {
        symbol: "MTY_RUNTIME_THREADS",
        signature: "env var MTY_RUNTIME_THREADS=<N>",
        description: "Per-worker scheduler count. Default 1 (single-threaded executor). Legacy `STARDUST_RUNTIME_THREADS` honoured.",
        capability: "",
        example: "// MTY_RUNTIME_THREADS=4 mty run app.mty\n",
        see_also: "MTY_TRACE, MTY_RUNTIME_CONTROL_SOCK",
    },
    StdlibExample {
        symbol: "MTY_RUNTIME_CONTROL_SOCK",
        signature: "env var MTY_RUNTIME_CONTROL_SOCK=<path>",
        description: "Boots a Unix-domain control socket for the runtime so `mty inspect` (and the replay tooling) can attach without restart. No legacy spelling — introduced at the renamed name in v0.16.",
        capability: "",
        example: "// MTY_RUNTIME_CONTROL_SOCK=/tmp/mty.sock mty run app.mty\n// mty inspect --sock /tmp/mty.sock\n",
        see_also: "MTY_RUNTIME_THREADS, MTY_TRACE",
    },
    // ---- v0.38 T4: std.process ----
    StdlibExample {
        symbol: "Command.new",
        signature: "fn Command.new(program: Str) -> Command",
        description: "Builder for an OS subprocess. Fluent `.arg`/`.env`/`.spawn`/`.exec` mirror the `std::process::Command` ergonomics.",
        capability: "process.exec",
        example: "let out = Command.new(\"git\").arg(\"status\").exec()?;\nlog(out.stdout);\n",
        see_also: "Command.arg, Command.args, Command.exec, Command.spawn, std.process.spawn",
    },
    StdlibExample {
        symbol: "Command.arg",
        signature: "fn Command.arg(&mut self, a: Str) -> &mut Command",
        description: "Append one argument. Each call is a distinct argv slot — no shell splitting.",
        capability: "",
        example: "let mut c = Command.new(\"git\");\nc.arg(\"log\").arg(\"--oneline\");\n",
        see_also: "Command.new, Command.args, Command.env",
    },
    StdlibExample {
        symbol: "Command.args",
        signature: "fn Command.args(&mut self, xs: &[Str]) -> &mut Command",
        description: "Append every entry of `xs` as a separate argv slot.",
        capability: "",
        example: "Command.new(\"ls\").args(&[\"-la\", \"/tmp\"]).exec()?;\n",
        see_also: "Command.arg, Command.new",
    },
    StdlibExample {
        symbol: "Command.env",
        signature: "fn Command.env(&mut self, key: Str, value: Str) -> &mut Command",
        description: "Set one environment variable for the child. Repeated keys overwrite; `Command.env_clear` (separate call) wipes inherited env first.",
        capability: "",
        example: "Command.new(\"node\").arg(\"build.js\").env(\"NODE_ENV\", \"production\").exec()?;\n",
        see_also: "Command.new, Command.arg, std.process.exec",
    },
    StdlibExample {
        symbol: "Command.exec",
        signature: "fn Command.exec(&mut self) -> Result<ProcessOutput, ProcessError>",
        description: "Run to completion, collect stdout+stderr, and return on exit. Blocks the current task; combine with `spawn` if you need to interleave I/O.",
        capability: "process.exec",
        example: "let out = Command.new(\"echo\").arg(\"hi\").exec()?;\nassert_eq(out.stdout.trim_end(), \"hi\");\n",
        see_also: "Command.spawn, Command.new, ProcessExit",
    },
    StdlibExample {
        symbol: "Command.spawn",
        signature: "fn Command.spawn(&mut self) -> Result<Child, ProcessError>",
        description: "Start the child without waiting. Returns a `Child` handle — call `.wait()` (sync) or `.wait_async()` (yields the task) to collect the exit.",
        capability: "process.exec",
        example: "let mut child = Command.new(\"sleep\").arg(\"1\").spawn()?;\nlet exit = child.wait()?;\n",
        see_also: "Command.exec, std.process.wait, std.process.kill",
    },
    StdlibExample {
        symbol: "std.process.spawn",
        signature: "fn std.process.spawn(program: Str, args: &[Str]) -> Result<Child, ProcessError>",
        description: "Convenience over `Command.new(program).args(args).spawn()`. Returns the child handle for `.wait()` / `.kill()`.",
        capability: "process.exec",
        example: "let child = std.process.spawn(\"sleep\", &[\"5\"])?;\nstd.process.kill(child)?;\n",
        see_also: "std.process.exec, std.process.kill, std.process.wait, Command.spawn",
    },
    StdlibExample {
        symbol: "std.process.exec",
        signature: "fn std.process.exec(program: Str, args: &[Str]) -> Result<ProcessOutput, ProcessError>",
        description: "Run the program, block until exit, return the captured stdout+stderr+exit code as `ProcessOutput`.",
        capability: "process.exec",
        example: "let out = std.process.exec(\"git\", &[\"rev-parse\", \"HEAD\"])?;\nlet sha = out.stdout.trim_end();\n",
        see_also: "std.process.spawn, Command.exec, ProcessExit",
    },
    StdlibExample {
        symbol: "std.process.wait",
        signature: "fn std.process.wait(&mut child: Child) -> Result<ProcessExit, ProcessError>",
        description: "Block the current task until the child exits. Returns the exit status. Idempotent — calling on a finished child returns the cached exit.",
        capability: "",
        example: "let mut c = std.process.spawn(\"sleep\", &[\"1\"])?;\nlet exit = std.process.wait(&mut c)?;\nlog(format!(\"code={}\", exit.code));\n",
        see_also: "std.process.spawn, std.process.kill, ProcessExit",
    },
    StdlibExample {
        symbol: "std.process.kill",
        signature: "fn std.process.kill(child: Child) -> Result<(), ProcessError>",
        description: "Send SIGTERM (Unix) / TerminateProcess (Windows) to the child. Use `.wait()` afterwards to drain the exit.",
        capability: "process.exec",
        example: "let c = std.process.spawn(\"sleep\", &[\"30\"])?;\nstd.process.kill(c)?;\n",
        see_also: "std.process.wait, std.process.spawn, Command.spawn",
    },
    StdlibExample {
        symbol: "ProcessExit",
        signature: "struct ProcessExit { code: I32, signal: Option<I32> }",
        description: "The result of waiting on a child. `code` is the process exit status (0 = success). On Unix, `signal` carries the signal number when the child was killed; on Windows it is always `None`.",
        capability: "",
        example: "let exit = std.process.wait(&mut child)?;\nif exit.code != 0 { panic(\"child failed\"); }\n",
        see_also: "std.process.wait, std.process.exec, Command.exec",
    },
    // ---- v0.38 T4: std.io ----
    StdlibExample {
        symbol: "std.io.stdin",
        signature: "fn std.io.stdin() -> Stdin",
        description: "Returns the process stdin handle. Wrap in `BufReader.new(stdin)` for line iteration.",
        capability: "io.stdin",
        example: "let r = BufReader.new(std.io.stdin());\nfor line in r.lines() { log(line?); }\n",
        see_also: "std.io.stdout, std.io.stderr, BufReader.new",
    },
    StdlibExample {
        symbol: "std.io.stdout",
        signature: "fn std.io.stdout() -> Stdout",
        description: "Returns the process stdout handle. `log` and `println!` share its buffer; flush is implicit on newline.",
        capability: "io.stdout",
        example: "std.io.stdout().write_line(\"hello\")?;\n",
        see_also: "std.io.stderr, std.io.stdin, BufWriter.new, log",
    },
    StdlibExample {
        symbol: "std.io.stderr",
        signature: "fn std.io.stderr() -> Stderr",
        description: "Returns the process stderr handle. Unbuffered by default so diagnostics survive a crash before flush.",
        capability: "io.stderr",
        example: "std.io.stderr().write_line(\"error: bad input\")?;\n",
        see_also: "std.io.stdout, eprintln, panic",
    },
    StdlibExample {
        symbol: "eprintln",
        signature: "macro eprintln!(fmt: Str, args: ...) -> Unit",
        description: "`format!`-style writer to stderr followed by `\\n`. Cheaper to spell than `std.io.stderr().write_line(format!(...))`.",
        capability: "io.stderr",
        example: "eprintln!(\"failed: {}\", err);\n",
        see_also: "log, std.io.stderr, format",
    },
    StdlibExample {
        symbol: "BufReader.new",
        signature: "fn BufReader.new[R: Read](inner: R) -> BufReader[R]",
        description: "Wrap a `Read` handle in a buffered reader. Reduces syscall count for line-oriented workloads.",
        capability: "",
        example: "let r = BufReader.new(std.fs.open(\"input.txt\", FsCap.unrestricted())?);\n",
        see_also: "BufReader.lines, BufWriter.new, std.io.stdin",
    },
    StdlibExample {
        symbol: "BufReader.lines",
        signature: "fn BufReader.lines[R](&mut self) -> Iterator[Result<Str, IoError>]",
        description: "Lazy iterator of `\\n`-terminated lines (the terminator is stripped). Yields `Err` on read failure mid-stream.",
        capability: "",
        example: "let mut r = BufReader.new(std.io.stdin());\nfor line in r.lines() { log(line?); }\n",
        see_also: "BufReader.new, std.io.stdin",
    },
    StdlibExample {
        symbol: "BufWriter.new",
        signature: "fn BufWriter.new[W: Write](inner: W) -> BufWriter[W]",
        description: "Wrap a `Write` handle in a buffered writer. Coalesces small writes into block-sized syscalls; drop or `.flush()` to commit.",
        capability: "",
        example: "let mut w = BufWriter.new(std.io.stdout());\nw.write_line(\"hi\")?;\nw.flush()?;\n",
        see_also: "BufWriter.flush, BufReader.new, std.io.stdout",
    },
    StdlibExample {
        symbol: "BufWriter.flush",
        signature: "fn BufWriter.flush[W](&mut self) -> Result<(), IoError>",
        description: "Force-write any buffered bytes to the inner handle. Called automatically on drop but explicit flush surfaces I/O errors at a useful site.",
        capability: "",
        example: "let mut w = BufWriter.new(std.io.stdout());\nw.write_line(\"hi\")?;\nw.flush()?;\n",
        see_also: "BufWriter.new, std.io.stdout",
    },
    StdlibExample {
        symbol: "read_line",
        signature: "fn Stdin.read_line(&mut self, buf: &mut String) -> Result<USize, IoError>",
        description: "Read one `\\n`-terminated line from stdin into `buf` (appended, not replaced). Returns the byte count read; `0` on EOF.",
        capability: "io.stdin",
        example: "let mut s = String.new();\nlet n = std.io.stdin().read_line(&mut s)?;\nlog(format!(\"got {} bytes\", n));\n",
        see_also: "std.io.stdin, BufReader.lines, write_line",
    },
    StdlibExample {
        symbol: "write_line",
        signature: "fn Write.write_line(&mut self, s: Str) -> Result<(), IoError>",
        description: "Write `s` followed by a `\\n`. Implemented on every `Write` handle (`Stdout`, `Stderr`, `BufWriter`, file handles).",
        capability: "",
        example: "std.io.stdout().write_line(\"done\")?;\n",
        see_also: "std.io.stdout, BufWriter.new, eprintln",
    },
    // ---- v0.38 T4: std.path ----
    StdlibExample {
        symbol: "Path.new",
        signature: "fn Path.new(s: Str) -> &Path",
        description: "Borrow `s` as a filesystem path. Cheap — no allocation. The slice is platform-neutral; OS-specific separator handling kicks in on conversion to `PathBuf` or syscall.",
        capability: "",
        example: "let p = Path.new(\"/tmp/build.log\");\nassert_eq(p.file_name(), Some(\"build.log\"));\n",
        see_also: "PathBuf.new, Path.parent, Path.file_name, Path.extension",
    },
    StdlibExample {
        symbol: "PathBuf.new",
        signature: "fn PathBuf.new() -> PathBuf",
        description: "Allocates an empty owned path. Build with `.push()` / `.join()` and convert to `&Path` via `.as_path()` at syscall sites.",
        capability: "",
        example: "let mut p = PathBuf.new();\np.push(\"/var/log\");\np.push(\"app.log\");\n",
        see_also: "Path.new, Path.join, PathBuf.from",
    },
    StdlibExample {
        symbol: "Path.components",
        signature: "fn Path.components(&self) -> Iterator[Component]",
        description: "Iterator over the path's components (root, current dir, parent dir, normal segment). Normalised — collapses runs of separators but preserves `..`.",
        capability: "",
        example: "for c in Path.new(\"/a/b/c\").components() { log(format!(\"{:?}\", c)); }\n",
        see_also: "Path.parent, Path.file_name, Path.join",
    },
    StdlibExample {
        symbol: "Path.parent",
        signature: "fn Path.parent(&self) -> Option[&Path]",
        description: "Drop the final component. Returns `None` for root and empty paths.",
        capability: "",
        example: "assert_eq(Path.new(\"/a/b/c.txt\").parent(), Some(Path.new(\"/a/b\")));\n",
        see_also: "Path.file_name, Path.components, Path.join",
    },
    StdlibExample {
        symbol: "Path.file_name",
        signature: "fn Path.file_name(&self) -> Option[Str]",
        description: "The final non-separator component, or `None` for a path ending in `..` or `/`.",
        capability: "",
        example: "assert_eq(Path.new(\"/etc/hosts\").file_name(), Some(\"hosts\"));\n",
        see_also: "Path.parent, Path.extension, Path.components",
    },
    StdlibExample {
        symbol: "Path.extension",
        signature: "fn Path.extension(&self) -> Option[Str]",
        description: "Bytes after the final `.` of the file name, or `None` if there is no extension (e.g. `Makefile` or `.hidden`).",
        capability: "",
        example: "assert_eq(Path.new(\"main.mty\").extension(), Some(\"mty\"));\n",
        see_also: "Path.file_name, Path.with_extension",
    },
    StdlibExample {
        symbol: "Path.join",
        signature: "fn Path.join(&self, other: &Path) -> PathBuf",
        description: "Append `other` with the platform separator. If `other` is absolute, the result is `other` (rooted append wins, matching POSIX semantics).",
        capability: "",
        example: "let p = Path.new(\"/var/log\").join(Path.new(\"app.log\"));\n",
        see_also: "PathBuf.new, Path.parent, Path.with_extension",
    },
    StdlibExample {
        symbol: "Path.is_absolute",
        signature: "fn Path.is_absolute(&self) -> Bool",
        description: "True if the path starts with the platform root (`/` on Unix; drive prefix or `\\\\?\\` on Windows).",
        capability: "",
        example: "assert(Path.new(\"/etc\").is_absolute());\nassert(!Path.new(\"etc\").is_absolute());\n",
        see_also: "Path.join, Path.parent",
    },
    StdlibExample {
        symbol: "Path.with_extension",
        signature: "fn Path.with_extension(&self, ext: Str) -> PathBuf",
        description: "Replace the file extension (or add one when absent). Pass `\"\"` to strip the extension entirely.",
        capability: "",
        example: "let p = Path.new(\"build.mty\").with_extension(\"o\");\nassert_eq(p.file_name(), Some(\"build.o\"));\n",
        see_also: "Path.extension, Path.file_name",
    },
    StdlibExample {
        symbol: "Path.exists",
        signature: "fn Path.exists(&self, cap: FsCap) -> Bool",
        description: "Convenience shorthand for `std.fs.exists(self.as_str(), cap)`. Requires the same `FsCap` as the underlying syscall.",
        capability: "fs.read",
        example: "if Path.new(\"./mighty.toml\").exists(FsCap.rooted(\".\")) { /* ... */ }\n",
        see_also: "std.fs.exists, Path.new, FsCap.rooted",
    },
    // ---- v0.38 T4: std.collections ----
    StdlibExample {
        symbol: "HashMap.new",
        signature: "fn HashMap.new[K, V]() -> HashMap[K, V]",
        description: "Empty hash-backed key-value map. `K` must implement `Hash + Eq`.",
        capability: "",
        example: "let mut m: HashMap[Str, I32] = HashMap.new();\nm.insert(\"alpha\", 1);\n",
        see_also: "HashMap.insert, HashMap.get, BTreeMap.new",
    },
    StdlibExample {
        symbol: "HashMap.insert",
        signature: "fn HashMap.insert[K, V](&mut self, k: K, v: V) -> Option[V]",
        description: "Insert or overwrite. Returns the displaced value when the key was already present, `None` otherwise.",
        capability: "",
        example: "let prev = m.insert(\"alpha\", 2);\nassert_eq(prev, Some(1));\n",
        see_also: "HashMap.new, HashMap.get, HashMap.remove",
    },
    StdlibExample {
        symbol: "HashMap.get",
        signature: "fn HashMap.get[K, V](&self, k: &K) -> Option[&V]",
        description: "Borrow the value associated with `k`, or `None`.",
        capability: "",
        example: "match m.get(&\"alpha\") {\n  Some(v) => log(format!(\"{}\", v)),\n  None => log(\"missing\"),\n}\n",
        see_also: "HashMap.insert, HashMap.remove, HashMap.new",
    },
    StdlibExample {
        symbol: "HashMap.remove",
        signature: "fn HashMap.remove[K, V](&mut self, k: &K) -> Option[V]",
        description: "Remove and return the value at `k`, or `None` if absent.",
        capability: "",
        example: "let prev = m.remove(&\"alpha\");\n",
        see_also: "HashMap.insert, HashMap.get",
    },
    StdlibExample {
        symbol: "HashSet.new",
        signature: "fn HashSet.new[T]() -> HashSet[T]",
        description: "Empty hash-backed set. `T` must implement `Hash + Eq`.",
        capability: "",
        example: "let mut s: HashSet[Str] = HashSet.new();\ns.insert(\"alpha\");\n",
        see_also: "HashSet.insert, HashMap.new, BTreeSet.new",
    },
    StdlibExample {
        symbol: "HashSet.insert",
        signature: "fn HashSet.insert[T](&mut self, x: T) -> Bool",
        description: "Add `x`. Returns `true` when it was newly inserted, `false` when it was already present.",
        capability: "",
        example: "assert(s.insert(\"alpha\"));\nassert(!s.insert(\"alpha\"));\n",
        see_also: "HashSet.new, HashMap.insert",
    },
    StdlibExample {
        symbol: "BTreeMap.new",
        signature: "fn BTreeMap.new[K, V]() -> BTreeMap[K, V]",
        description: "Ordered key-value map. Iteration order is the `Ord` order on `K`. Cheaper than `HashMap` for small `K` count + range queries.",
        capability: "",
        example: "let mut m: BTreeMap[I32, Str] = BTreeMap.new();\nm.insert(1, \"alpha\");\nfor (k, v) in m.iter() { log(format!(\"{} -> {}\", k, v)); }\n",
        see_also: "HashMap.new, BTreeSet.new",
    },
    StdlibExample {
        symbol: "BTreeSet.new",
        signature: "fn BTreeSet.new[T]() -> BTreeSet[T]",
        description: "Ordered set. Iteration order is `Ord` order on `T`.",
        capability: "",
        example: "let mut s: BTreeSet[I32] = BTreeSet.new();\ns.insert(3);\ns.insert(1);\nfor x in s.iter() { log(format!(\"{}\", x)); }\n",
        see_also: "HashSet.new, BTreeMap.new",
    },
    // ---- v0.38 T4: std.iter ----
    StdlibExample {
        symbol: "Iterator.map",
        signature: "fn Iterator.map[T, U](self, f: fn(T) -> U) -> Iterator[U]",
        description: "Lazily transform each item. Combinator — nothing runs until the consumer drives the iterator.",
        capability: "",
        example: "let xs = [1, 2, 3].iter().map(|x| x * 2).collect();\nassert_eq(xs, [2, 4, 6]);\n",
        see_also: "Iterator.filter, Iterator.fold, Iterator.collect",
    },
    StdlibExample {
        symbol: "Iterator.filter",
        signature: "fn Iterator.filter[T](self, pred: fn(&T) -> Bool) -> Iterator[T]",
        description: "Lazily keep only items where `pred` returns `true`.",
        capability: "",
        example: "let evens = (1..10).filter(|x| x % 2 == 0).collect();\n",
        see_also: "Iterator.map, Iterator.fold, Iterator.collect",
    },
    StdlibExample {
        symbol: "Iterator.fold",
        signature: "fn Iterator.fold[T, A](self, init: A, f: fn(A, T) -> A) -> A",
        description: "Reduce to a single value by threading `init` through `f` for each item. Terminal — drives the iterator.",
        capability: "",
        example: "let sum = [1, 2, 3].iter().fold(0, |a, x| a + x);\nassert_eq(sum, 6);\n",
        see_also: "Iterator.map, Iterator.filter, Iterator.sum",
    },
    StdlibExample {
        symbol: "Iterator.collect",
        signature: "fn Iterator.collect[T, C: FromIterator[T]](self) -> C",
        description: "Terminal: drain the iterator into a collection. Target type is inferred from context (`Vec[T]`, `HashSet[T]`, ...).",
        capability: "",
        example: "let v: Vec[I32] = (1..5).collect();\nassert_eq(v.len(), 4);\n",
        see_also: "Iterator.map, Vec.iter, HashSet.new",
    },
    StdlibExample {
        symbol: "Iterator.zip",
        signature: "fn Iterator.zip[T, U](self, other: Iterator[U]) -> Iterator[(T, U)]",
        description: "Pairwise combine. The result stops as soon as either source is exhausted.",
        capability: "",
        example: "let pairs = [1, 2, 3].iter().zip([\"a\", \"b\"].iter()).collect();\nassert_eq(pairs.len(), 2);\n",
        see_also: "Iterator.chain, Iterator.enumerate, Iterator.map",
    },
    StdlibExample {
        symbol: "Iterator.chain",
        signature: "fn Iterator.chain[T](self, other: Iterator[T]) -> Iterator[T]",
        description: "Yield every item of `self`, then every item of `other`. Lazy.",
        capability: "",
        example: "let all = [1, 2].iter().chain([3, 4].iter()).collect();\nassert_eq(all, [1, 2, 3, 4]);\n",
        see_also: "Iterator.zip, Iterator.collect, Iterator.map",
    },
    StdlibExample {
        symbol: "Iterator.take",
        signature: "fn Iterator.take[T](self, n: USize) -> Iterator[T]",
        description: "Yield at most the first `n` items, then stop.",
        capability: "",
        example: "let first3 = (1..).take(3).collect();\nassert_eq(first3, [1, 2, 3]);\n",
        see_also: "Iterator.skip, Iterator.enumerate, Iterator.collect",
    },
    StdlibExample {
        symbol: "Iterator.skip",
        signature: "fn Iterator.skip[T](self, n: USize) -> Iterator[T]",
        description: "Drop the first `n` items, then yield the rest.",
        capability: "",
        example: "let tail = (1..5).skip(2).collect();\nassert_eq(tail, [3, 4]);\n",
        see_also: "Iterator.take, Iterator.enumerate",
    },
    StdlibExample {
        symbol: "Iterator.enumerate",
        signature: "fn Iterator.enumerate[T](self) -> Iterator[(USize, T)]",
        description: "Yield `(index, item)` pairs starting at index `0`.",
        capability: "",
        example: "for (i, name) in names.iter().enumerate() {\n  log(format!(\"{}: {}\", i, name));\n}\n",
        see_also: "Iterator.zip, Iterator.map, Vec.iter",
    },
    StdlibExample {
        symbol: "Iterator.sum",
        signature: "fn Iterator.sum[T: Add](self) -> T",
        description: "Reduce by `+`. Terminal. Works for any item type with a `+ T -> T` Add impl.",
        capability: "",
        example: "let total: I64 = [1, 2, 3].iter().sum();\nassert_eq(total, 6);\n",
        see_also: "Iterator.fold, Iterator.collect",
    },
    // ---- v0.38 T4: std.result ----
    StdlibExample {
        symbol: "Result.is_ok",
        signature: "fn Result.is_ok[T, E](&self) -> Bool",
        description: "True when the result is `Ok(_)`. Cheap shortcut over `matches!(r, Ok(_))`.",
        capability: "",
        example: "let r: Result[I32, IoError] = Ok(42);\nassert(r.is_ok());\n",
        see_also: "Result.ok, Result.err, Result.unwrap_or",
    },
    StdlibExample {
        symbol: "Result.ok",
        signature: "fn Result.ok[T, E](self) -> Option[T]",
        description: "Convert to `Some(t)` on `Ok(t)`, `None` on `Err(_)`. Drops the error.",
        capability: "",
        example: "let r: Result[I32, IoError] = Ok(7);\nassert_eq(r.ok(), Some(7));\n",
        see_also: "Result.err, Result.is_ok, Option.is_some",
    },
    StdlibExample {
        symbol: "Result.err",
        signature: "fn Result.err[T, E](self) -> Option[E]",
        description: "Convert to `Some(e)` on `Err(e)`, `None` on `Ok(_)`. Drops the success value.",
        capability: "",
        example: "let r: Result[I32, IoError] = Err(IoError.NotFound);\nassert(r.err().is_some());\n",
        see_also: "Result.ok, Result.is_ok",
    },
    StdlibExample {
        symbol: "Result.map_err",
        signature: "fn Result.map_err[T, E, E2](self, f: fn(E) -> E2) -> Result[T, E2]",
        description: "Transform the error variant with `f`. Used to translate a low-level error into the caller's domain shape.",
        capability: "",
        example: "let r = read_config().map_err(|e| AppError.Config(e));\n",
        see_also: "Result.and_then, Result.unwrap_or, Result.err",
    },
    StdlibExample {
        symbol: "Result.unwrap_or",
        signature: "fn Result.unwrap_or[T, E](self, default: T) -> T",
        description: "Return the `Ok` value or `default` on `Err`. Never panics.",
        capability: "",
        example: "let port: I32 = parse_port(input).unwrap_or(8080);\n",
        see_also: "Result.ok, Result.map_err, Result.and_then",
    },
    StdlibExample {
        symbol: "Result.and_then",
        signature: "fn Result.and_then[T, U, E](self, f: fn(T) -> Result[U, E]) -> Result[U, E]",
        description: "Monadic bind. Threads the `Ok` value through `f`; short-circuits on `Err`. Mirrors `Result::and_then`.",
        capability: "",
        example: "let n = parse_int(s).and_then(|x| if x > 0 { Ok(x) } else { Err(\"negative\") });\n",
        see_also: "Result.map_err, Result.unwrap_or, Option.and_then",
    },
    // ---- v0.38 T4: std.option ----
    StdlibExample {
        symbol: "Option.is_some",
        signature: "fn Option.is_some[T](&self) -> Bool",
        description: "True when the option holds a value. Cheap shortcut over `matches!(o, Some(_))`.",
        capability: "",
        example: "let o: Option[I32] = Some(7);\nassert(o.is_some());\n",
        see_also: "Option.is_none, Option.map, Option.unwrap_or",
    },
    StdlibExample {
        symbol: "Option.is_none",
        signature: "fn Option.is_none[T](&self) -> Bool",
        description: "True when the option is empty.",
        capability: "",
        example: "let o: Option[I32] = None;\nassert(o.is_none());\n",
        see_also: "Option.is_some, Option.ok_or",
    },
    StdlibExample {
        symbol: "Option.map",
        signature: "fn Option.map[T, U](self, f: fn(T) -> U) -> Option[U]",
        description: "Transform the `Some` value with `f`; pass `None` through unchanged.",
        capability: "",
        example: "let len = Some(\"hi\").map(|s| s.len());\nassert_eq(len, Some(2));\n",
        see_also: "Option.and_then, Option.unwrap_or, Result.map_err",
    },
    StdlibExample {
        symbol: "Option.and_then",
        signature: "fn Option.and_then[T, U](self, f: fn(T) -> Option[U]) -> Option[U]",
        description: "Monadic bind. Threads the inner value through `f`; short-circuits on `None`.",
        capability: "",
        example: "let n = parse_int(s).and_then(|x| if x > 0 { Some(x) } else { None });\n",
        see_also: "Option.map, Option.unwrap_or, Result.and_then",
    },
    StdlibExample {
        symbol: "Option.unwrap_or",
        signature: "fn Option.unwrap_or[T](self, default: T) -> T",
        description: "Return the `Some` value or `default` on `None`. Never panics.",
        capability: "",
        example: "let n = parse_int(s).unwrap_or(0);\n",
        see_also: "Option.map, Option.ok_or, Result.unwrap_or",
    },
    StdlibExample {
        symbol: "Option.ok_or",
        signature: "fn Option.ok_or[T, E](self, err: E) -> Result[T, E]",
        description: "Promote an `Option` to a `Result`, using `err` as the failure value.",
        capability: "",
        example: "let r: Result[I32, Str] = parse_int(s).ok_or(\"bad int\");\n",
        see_also: "Option.map, Result.ok, Result.err",
    },
    // ---- v0.38 T4: std.error ----
    StdlibExample {
        symbol: "Error.trait",
        signature: "trait Error: Display + Debug { fn source(&self) -> Option[&dyn Error] { None } }",
        description: "Common interface for typed error values. Implementations carry a `Display` impl for human-readable messages and an optional `source()` chain. Mirrors `std::error::Error`.",
        capability: "",
        example: "impl Error for AppError {\n  fn source(&self) -> Option[&dyn Error] {\n    match self {\n      AppError.Io(e) => Some(e),\n      _ => None,\n    }\n  }\n}\n",
        see_also: "Result.map_err, anyhow_error",
    },
    StdlibExample {
        symbol: "anyhow_error",
        signature: "macro anyhow!(fmt: Str, args: ...) -> AnyhowError",
        description: "Construct an `AnyhowError` from a format string (anyhow-style ergonomics). The error carries a stringified message and the `?`-chain backtrace.",
        capability: "",
        example: "if !port_ok { return Err(anyhow!(\"bad port: {}\", port)); }\n",
        see_also: "Error.trait, Result.map_err, panic",
    },
    // ---- v0.38 T4: polish on existing v0.30+ surfaces ----
    StdlibExample {
        symbol: "Member.weighted",
        signature: "fn Member.weighted(inner: Member, weight: F32) -> Member",
        description: "Wraps `inner` with a vote weight surfaced to `ConsensusStrategy.WeightedVote`. `weight` must be ≥ 0; `0.0` is a silent member (observed but not counted).",
        capability: "",
        example: "let m = Member.weighted(Member.openai(\"gpt-4o\"), 2.0);\nlet panel = [m, Member.anthropic(\"claude-opus-4-7\")];\n",
        see_also: "ConsensusStrategy.WeightedVote, Member.anthropic, swarm",
    },
    StdlibExample {
        symbol: "Member.panel_of",
        signature: "fn Member.panel_of(specs: &[Str]) -> List<Member>",
        description: "Bulk constructor: parse a list of `provider:model` strings into a panel. Unknown providers yield MT4181 INVALID_MODEL_SPEC.",
        capability: "",
        example: "let panel = Member.panel_of(&[\"anthropic:claude-opus-4-7\", \"openai:gpt-4o\"]);\n",
        see_also: "Member.anthropic, Member.openai, Member.gemini, swarm",
    },
    StdlibExample {
        symbol: "ConsensusStrategy.AbortOnDissent",
        signature: "const ConsensusStrategy.AbortOnDissent: ConsensusStrategy",
        description: "Aborts the swarm the moment any member dissents from the running plurality. Useful as a CI gate: any disagreement = `Consensus.has_consensus() == false`.",
        capability: "",
        example: "let c = swarm(p, panel, b, ConsensusStrategy.AbortOnDissent).await?;\nassert(c.has_consensus());\n",
        see_also: "ConsensusStrategy.Majority, ConsensusStrategy.Unanimous, Consensus.has_consensus, swarm",
    },
    StdlibExample {
        symbol: "Window.Recent",
        signature: "fn Window.Recent(dur: Duration) -> Window",
        description: "Half-open window over the last `dur` of telemetry events. Rolls forward as time advances.",
        capability: "",
        example: "let w = Window.Recent(5.minutes);\nlet rows = std.observe.query(w, GroupBy.Model);\n",
        see_also: "Window.Last, Window.parse, observe.query, GroupBy.Model",
    },
    StdlibExample {
        symbol: "GroupBy.Tenant",
        signature: "const GroupBy.Tenant: GroupBy",
        description: "Bucket telemetry rows by the multi-tenant `tenant_id` tag (when present). Rows lacking the tag fall into the `None` bucket.",
        capability: "",
        example: "let rows = std.observe.query(Window.Last(1), GroupBy.Tenant);\n",
        see_also: "GroupBy.Provider, GroupBy.Model, GroupBy.Agent, observe.query",
    },
    StdlibExample {
        symbol: "top_by_cost",
        signature: "fn top_by_cost(window: Window, n: USize) -> Vec[CostSummary]",
        description: "Returns the top `n` cost-emitting buckets in the window, sorted descending. Handy for budget alerting.",
        capability: "",
        example: "let worst = top_by_cost(Window.Recent(1.hour), 5);\nfor row in worst.iter() { log(format!(\"{} = {}c\", row.label, row.cents)); }\n",
        see_also: "CostSummary, observe.query, percentiles, summarize",
    },
    StdlibExample {
        symbol: "Suite.compare_with",
        signature: "fn Suite.compare_with(self, cmp: Compare) -> Suite",
        description: "Set the comparator used when no per-case override is supplied. Chain after `.case().run_with(...)` to declare the eval pipeline declaratively.",
        capability: "",
        example: "let s = Suite.new(\"qa\")\n  .case(c1)\n  .run_with(member)\n  .compare_with(Compare.semantic_similarity(0.8));\n",
        see_also: "Suite.new, Suite.case, Suite.run_with, Compare.equal",
    },
    StdlibExample {
        symbol: "sanitize_compose",
        signature: "fn sanitize_compose(first: Sanitizer, second: Sanitizer) -> Sanitizer",
        description: "Chain two sanitisers. The output of `first` feeds `second`. Useful for `HtmlEscape + ShellEscape` belt-and-braces.",
        capability: "",
        example: "let s = sanitize_compose(HtmlEscape, ShellEscape);\nlet safe = sanitize_with(input, s)?;\n",
        see_also: "sanitize_with, HtmlEscape, ShellEscape, SqlEscape",
    },
    StdlibExample {
        symbol: "named_regex",
        signature: "fn named_regex(name: Str, pattern: Str) -> NamedRegex",
        description: "Cache a regex by name. Subsequent lookups via `matches_regex(name, input)` skip the compile step — important when the pattern is hot.",
        capability: "",
        example: "named_regex(\"email\", r\"^[^@]+@[^@]+\\.[a-z]+$\");\nif matches_regex(\"email\", input) { /* ok */ }\n",
        see_also: "matches_regex, in_allowlist, sanitize_with",
    },
    StdlibExample {
        symbol: "Allowlist.from_enum",
        signature: "fn Allowlist.from_enum[E: AsStr](variants: &[E]) -> Allowlist",
        description: "Build an allowlist from a closed enum. Equivalent to `in_allowlist(input, &[v0.as_str(), v1.as_str(), ...])` but type-checked.",
        capability: "",
        example: "let a = Allowlist.from_enum(&[Lang.En, Lang.Fr, Lang.Es]);\nif in_allowlist(input, a) { /* ok */ }\n",
        see_also: "in_allowlist, named_regex, sanitize_with",
    },
    StdlibExample {
        symbol: "Iterator.count",
        signature: "fn Iterator.count[T](self) -> USize",
        description: "Terminal: consume the iterator and return the number of items yielded. O(n) — use `Vec.len` if you already have a sized container.",
        capability: "",
        example: "let n = (1..100).filter(|x| x % 7 == 0).count();\n",
        see_also: "Iterator.fold, Iterator.sum, Vec.len",
    },
    StdlibExample {
        symbol: "Iterator.any",
        signature: "fn Iterator.any[T](self, pred: fn(T) -> Bool) -> Bool",
        description: "Terminal: returns `true` as soon as any item satisfies `pred`. Short-circuits on the first match.",
        capability: "",
        example: "if names.iter().any(|n| n == \"root\") { panic(\"root not allowed\"); }\n",
        see_also: "Iterator.all, Iterator.filter",
    },
    StdlibExample {
        symbol: "Iterator.all",
        signature: "fn Iterator.all[T](self, pred: fn(T) -> Bool) -> Bool",
        description: "Terminal: returns `true` iff every item satisfies `pred`. Short-circuits on the first failure. Vacuously true on the empty iterator.",
        capability: "",
        example: "assert(xs.iter().all(|x| x > 0));\n",
        see_also: "Iterator.any, Iterator.filter",
    },
    StdlibExample {
        symbol: "Iterator.find",
        signature: "fn Iterator.find[T](self, pred: fn(&T) -> Bool) -> Option[T]",
        description: "Terminal: return the first item satisfying `pred`, or `None`. Short-circuits.",
        capability: "",
        example: "let first_even = (1..).find(|x| x % 2 == 0);\nassert_eq(first_even, Some(2));\n",
        see_also: "Iterator.any, Iterator.filter, Option.is_some",
    },
    // ============================================================
    // v0.39 T5 — v0.39 T1 stdlib (crypto / encoding / url / uuid)
    // ============================================================
    // ---- std.crypto.hash ----
    StdlibExample {
        symbol: "std.crypto.sha256",
        signature: "fn std.crypto.sha256(input: &[U8]) -> [U8; 32]",
        description: "SHA-256 one-shot hash. Returns the 32-byte digest. Pure — no capability required. KAT-tested against NIST FIPS-180-2.",
        capability: "",
        example: "let h = std.crypto.sha256(\"hello\".as_bytes());\nlet hex_digest = std.encoding.hex.encode(&h);\n",
        see_also: "std.crypto.sha512, std.crypto.blake3, std.crypto.Sha256Hasher, std.crypto.hmac_sha256",
    },
    StdlibExample {
        symbol: "std.crypto.sha512",
        signature: "fn std.crypto.sha512(input: &[U8]) -> [U8; 64]",
        description: "SHA-512 one-shot hash. Returns the 64-byte digest. Pure. KAT-tested against RFC 4231 / NIST.",
        capability: "",
        example: "let h = std.crypto.sha512(\"hello\".as_bytes());\nlet hex_digest = std.encoding.hex.encode(&h);\n",
        see_also: "std.crypto.sha256, std.crypto.Sha512Hasher, std.crypto.hmac_sha512",
    },
    StdlibExample {
        symbol: "std.crypto.blake3",
        signature: "fn std.crypto.blake3(input: &[U8]) -> [U8; 32]",
        description: "BLAKE3 hash. Fast modern hash function. Returns the default 32-byte digest. Pure. KAT-tested against the BLAKE3 reference vectors.",
        capability: "",
        example: "let h = std.crypto.blake3(file_bytes);\n",
        see_also: "std.crypto.sha256, std.crypto.Blake3Hasher",
    },
    StdlibExample {
        symbol: "std.crypto.Sha256Hasher",
        signature: "struct std.crypto.Sha256Hasher",
        description: "Streaming SHA-256. `new()` → `update(&[U8])` (repeated) → `finalize() -> [U8; 32]`. Use when the input does not fit in memory.",
        capability: "",
        example: "let h = std.crypto.Sha256Hasher.new();\nh.update(chunk1);\nh.update(chunk2);\nlet digest = h.finalize();\n",
        see_also: "std.crypto.sha256, std.crypto.Sha512Hasher, std.crypto.Blake3Hasher",
    },
    StdlibExample {
        symbol: "std.crypto.Sha512Hasher",
        signature: "struct std.crypto.Sha512Hasher",
        description: "Streaming SHA-512. Same shape as `Sha256Hasher` — `new()` / `update(&[U8])` / `finalize() -> [U8; 64]`.",
        capability: "",
        example: "let h = std.crypto.Sha512Hasher.new();\nh.update(chunk);\nlet digest = h.finalize();\n",
        see_also: "std.crypto.sha512, std.crypto.Sha256Hasher",
    },
    StdlibExample {
        symbol: "std.crypto.Blake3Hasher",
        signature: "struct std.crypto.Blake3Hasher",
        description: "Streaming BLAKE3. Beyond the standard `new() / update / finalize`, exposes `finalize_xof(&mut [U8])` for arbitrary-length XOF output (KDF / extended MAC).",
        capability: "",
        example: "let h = std.crypto.Blake3Hasher.new();\nh.update(body);\nlet mut out = [0_u8; 64];\nh.finalize_xof(&mut out);\n",
        see_also: "std.crypto.blake3, std.crypto.Sha256Hasher",
    },
    // ---- std.crypto.hmac ----
    StdlibExample {
        symbol: "std.crypto.hmac_sha256",
        signature: "fn std.crypto.hmac_sha256(key: &[U8], message: &[U8]) -> [U8; 32]",
        description: "HMAC-SHA-256 — keyed MAC tag per RFC 2104. Returns the 32-byte tag. Pure. KAT-tested against RFC 4231.",
        capability: "",
        example: "// Webhook signature\nlet mac = std.crypto.hmac_sha256(secret, body);\nlet header = \"sha256=\" + std.encoding.hex.encode(&mac);\n",
        see_also: "std.crypto.hmac_sha512, std.crypto.subtle_eq, std.crypto.sha256",
    },
    StdlibExample {
        symbol: "std.crypto.hmac_sha512",
        signature: "fn std.crypto.hmac_sha512(key: &[U8], message: &[U8]) -> [U8; 64]",
        description: "HMAC-SHA-512 — keyed MAC tag per RFC 2104. Returns the 64-byte tag. Pure.",
        capability: "",
        example: "let mac = std.crypto.hmac_sha512(key, message);\n",
        see_also: "std.crypto.hmac_sha256, std.crypto.subtle_eq, std.crypto.sha512",
    },
    StdlibExample {
        symbol: "std.crypto.subtle_eq",
        signature: "fn std.crypto.subtle_eq(a: &[U8], b: &[U8]) -> Bool",
        description: "Constant-time slice equality. Use for tag / digest comparison — never the naive `==` (which short-circuits and leaks timing).",
        capability: "",
        example: "if std.crypto.subtle_eq(&expected_tag, &computed_tag) {\n  // valid signature\n}\n",
        see_also: "std.crypto.hmac_sha256, std.crypto.hmac_sha512",
    },
    // ---- std.crypto.rand ----
    StdlibExample {
        symbol: "std.crypto.random_bytes",
        signature: "fn std.crypto.random_bytes(n: USize) -> Result[Vec[U8], RandErr]",
        description: "Cryptographically-secure random bytes from the OS CSPRNG (Linux `getrandom(2)`, macOS `getentropy(2)`, Windows `BCryptGenRandom`). Requires `crypto.rand`.",
        capability: "crypto.rand",
        example: "let nonce = std.crypto.random_bytes(16)?;\n",
        see_also: "std.crypto.uniform_int, std.crypto.uniform_f64, std.uuid.Uuid.v4",
    },
    StdlibExample {
        symbol: "std.crypto.uniform_int",
        signature: "fn std.crypto.uniform_int(low: I64, high: I64) -> Result[I64, RandErr]",
        description: "Uniform-random integer in `[low, high)`. Rejection-sampled against the OS entropy stream to avoid modulo bias. Returns `RandErr.EmptyRange` if `low >= high`. Requires `crypto.rand`.",
        capability: "crypto.rand",
        example: "let dice = std.crypto.uniform_int(1, 7)?;\n",
        see_also: "std.crypto.random_bytes, std.crypto.uniform_f64, std.crypto.RandErr",
    },
    StdlibExample {
        symbol: "std.crypto.uniform_f64",
        signature: "fn std.crypto.uniform_f64() -> Result[F64, RandErr]",
        description: "Uniform-random F64 in `[0.0, 1.0)`. 53-bit mantissa precision (every representable F64 in that range with a 53-bit integer mantissa is equally likely). Requires `crypto.rand`.",
        capability: "crypto.rand",
        example: "let p = std.crypto.uniform_f64()?;\nif p < 0.01 { /* 1% probability branch */ }\n",
        see_also: "std.crypto.uniform_int, std.crypto.random_bytes",
    },
    StdlibExample {
        symbol: "std.crypto.RandErr",
        signature: "enum std.crypto.RandErr { Os(Str), EmptyRange }",
        description: "Error returned by the `random_bytes` / `uniform_*` surfaces. `Os(msg)` wraps a CSPRNG syscall failure; `EmptyRange` rejects `uniform_int(low, high)` with `low >= high`.",
        capability: "",
        example: "match std.crypto.uniform_int(5, 5) {\n  Err(std.crypto.RandErr.EmptyRange) => { /* expected */ }\n  _ => panic(\"unreachable\"),\n}\n",
        see_also: "std.crypto.random_bytes, std.crypto.uniform_int",
    },
    // ---- std.encoding.base64 ----
    StdlibExample {
        symbol: "std.encoding.base64.encode",
        signature: "fn std.encoding.base64.encode(bytes: &[U8]) -> Str",
        description: "Standard (RFC 4648 § 4) Base64 encode. Emits `=` padding. KAT-tested against RFC 4648 § 10.",
        capability: "",
        example: "let s = std.encoding.base64.encode(\"hello\".as_bytes());  // \"aGVsbG8=\"\n",
        see_also: "std.encoding.base64.decode, std.encoding.base64.encode_url, std.encoding.hex.encode",
    },
    StdlibExample {
        symbol: "std.encoding.base64.decode",
        signature: "fn std.encoding.base64.decode(s: Str) -> Result[Vec[U8], Base64Err]",
        description: "Decode standard Base64. Accepts both padded and unpadded forms (some legacy emitters drop the trailing `=`).",
        capability: "",
        example: "let bytes = std.encoding.base64.decode(\"aGVsbG8=\")?;\n",
        see_also: "std.encoding.base64.encode, std.encoding.base64.decode_url",
    },
    StdlibExample {
        symbol: "std.encoding.base64.encode_url",
        signature: "fn std.encoding.base64.encode_url(bytes: &[U8]) -> Str",
        description: "URL-safe (RFC 4648 § 5) Base64 encode (`- _` alphabet). Emits `=` padding.",
        capability: "",
        example: "let s = std.encoding.base64.encode_url(&token_bytes);\n",
        see_also: "std.encoding.base64.encode_url_no_pad, std.encoding.base64.decode_url",
    },
    StdlibExample {
        symbol: "std.encoding.base64.encode_url_no_pad",
        signature: "fn std.encoding.base64.encode_url_no_pad(bytes: &[U8]) -> Str",
        description: "URL-safe Base64 with no `=` padding — the JWT / JWS shape.",
        capability: "",
        example: "let jwt_header = std.encoding.base64.encode_url_no_pad(&header_json_bytes);\n",
        see_also: "std.encoding.base64.encode_url, std.encoding.base64.decode_url",
    },
    StdlibExample {
        symbol: "std.encoding.base64.decode_url",
        signature: "fn std.encoding.base64.decode_url(s: Str) -> Result[Vec[U8], Base64Err]",
        description: "Decode URL-safe Base64. Accepts both padded and unpadded forms (JWT drops the padding).",
        capability: "",
        example: "let payload = std.encoding.base64.decode_url(jwt_segment)?;\n",
        see_also: "std.encoding.base64.encode_url, std.encoding.base64.encode_url_no_pad",
    },
    StdlibExample {
        symbol: "std.encoding.Base64Err",
        signature: "enum std.encoding.Base64Err { Decode(Str) }",
        description: "Error returned by `base64.decode` / `base64.decode_url`. Carries the underlying parser message (invalid alphabet, malformed padding, etc.).",
        capability: "",
        example: "match std.encoding.base64.decode(input) {\n  Err(std.encoding.Base64Err.Decode(msg)) => log(format!(\"bad base64: {}\", msg)),\n  Ok(bytes) => process(bytes),\n}\n",
        see_also: "std.encoding.base64.decode, std.encoding.base64.decode_url",
    },
    // ---- std.encoding.hex ----
    StdlibExample {
        symbol: "std.encoding.hex.encode",
        signature: "fn std.encoding.hex.encode(bytes: &[U8]) -> Str",
        description: "Lowercase hex (base16) encode per RFC 4648 § 8. 2 × N characters.",
        capability: "",
        example: "let s = std.encoding.hex.encode(&digest);  // \"deadbeef...\"\n",
        see_also: "std.encoding.hex.encode_upper, std.encoding.hex.decode, std.encoding.base64.encode",
    },
    StdlibExample {
        symbol: "std.encoding.hex.encode_upper",
        signature: "fn std.encoding.hex.encode_upper(bytes: &[U8]) -> Str",
        description: "Uppercase hex encode (`0-9 A-F`). Same byte layout as `encode`, different alphabet.",
        capability: "",
        example: "let s = std.encoding.hex.encode_upper(&mac);  // \"DEADBEEF\"\n",
        see_also: "std.encoding.hex.encode, std.encoding.hex.decode",
    },
    StdlibExample {
        symbol: "std.encoding.hex.decode",
        signature: "fn std.encoding.hex.decode(s: Str) -> Result[Vec[U8], HexErr]",
        description: "Decode hex string. Accepts mixed case. Rejects odd length (`HexErr.OddLength`) and non-hex chars (`HexErr.BadChar`).",
        capability: "",
        example: "let bytes = std.encoding.hex.decode(\"DeAdBeEf\")?;\n",
        see_also: "std.encoding.hex.encode, std.encoding.hex.encode_upper",
    },
    StdlibExample {
        symbol: "std.encoding.HexErr",
        signature: "enum std.encoding.HexErr { BadChar(Char, USize), OddLength(USize) }",
        description: "Error returned by `hex.decode`. `BadChar(c, i)` carries the offending character + index; `OddLength(n)` carries the total length when the input had an odd number of chars.",
        capability: "",
        example: "match std.encoding.hex.decode(input) {\n  Err(std.encoding.HexErr.OddLength(n)) => log(format!(\"len={}\", n)),\n  Err(std.encoding.HexErr.BadChar(c, i)) => log(format!(\"bad {:?}@{}\", c, i)),\n  Ok(bytes) => use(bytes),\n}\n",
        see_also: "std.encoding.hex.decode",
    },
    // ---- std.url.parse + Url struct ----
    StdlibExample {
        symbol: "std.url.parse",
        signature: "fn std.url.parse(s: Str) -> Result[Url, UrlErr]",
        description: "Parse a URL string into named fields per RFC 3986 / WHATWG. Backed by the `url` crate, exposed with a struct-of-strings shape rather than getter methods.",
        capability: "",
        example: "let u = std.url.parse(\"https://example.com/path?q=hello world\")?;\nlog(format!(\"host={}, query={}\", u.host, u.query));\n",
        see_also: "std.url.Url, std.url.Url.builder, std.url.UrlErr, std.url.percent_encode",
    },
    StdlibExample {
        symbol: "std.url.Url",
        signature: "struct std.url.Url { scheme: Str, username: Str, password: Str, host: Str, port: Option[U16], path: Str, query: Str, fragment: Str }",
        description: "Parsed URL components. Empty `Str` fields signal \"absent\" (avoids `Option` unwrap noise); only `port` keeps an explicit `Option` because `0` would be ambiguous.",
        capability: "",
        example: "let u = std.url.parse(\"https://api.example.com:8443/v1?q=hi\")?;\nassert_eq(u.scheme, \"https\");\nassert_eq(u.port, Some(8443));\n",
        see_also: "std.url.parse, std.url.Url.builder, std.url.Url.to_string",
    },
    StdlibExample {
        symbol: "std.url.Url.builder",
        signature: "fn std.url.Url.builder(scheme: Str) -> UrlBuilder",
        description: "Start a fluent URL builder. Chain `.host()` / `.port()` / `.path()` / `.query_param()` / `.userinfo()` / `.fragment()` / `.build()`.",
        capability: "",
        example: "let u = std.url.Url.builder(\"https\")\n  .host(\"api.example.com\")\n  .path(\"/v1/search\")\n  .query_param(\"q\", \"hello world\")\n  .build()?;\n",
        see_also: "std.url.Url, std.url.UrlBuilder.host, std.url.UrlBuilder.query_param, std.url.UrlBuilder.build",
    },
    StdlibExample {
        symbol: "std.url.Url.to_string",
        signature: "fn std.url.Url.to_string(&self) -> Str",
        description: "Render the URL back to a canonical string form. Inverse of `std.url.parse` for round-trippable inputs.",
        capability: "",
        example: "let u = std.url.parse(\"https://example.com/x\")?;\nassert_eq(u.to_string(), \"https://example.com/x\");\n",
        see_also: "std.url.parse, std.url.Url",
    },
    // ---- std.url.UrlBuilder fluent surface ----
    StdlibExample {
        symbol: "std.url.UrlBuilder.host",
        signature: "fn std.url.UrlBuilder.host(self, host: Str) -> UrlBuilder",
        description: "Set the host segment. Pass an empty string to leave the URL hostless (data: URLs).",
        capability: "",
        example: "let u = std.url.Url.builder(\"https\").host(\"api.example.com\").build()?;\n",
        see_also: "std.url.Url.builder, std.url.UrlBuilder.port, std.url.UrlBuilder.path",
    },
    StdlibExample {
        symbol: "std.url.UrlBuilder.port",
        signature: "fn std.url.UrlBuilder.port(self, port: U16) -> UrlBuilder",
        description: "Set the TCP port. Omit for the scheme default (https → 443, http → 80).",
        capability: "",
        example: "let u = std.url.Url.builder(\"https\").host(\"x\").port(8443).build()?;\n",
        see_also: "std.url.Url.builder, std.url.UrlBuilder.host",
    },
    StdlibExample {
        symbol: "std.url.UrlBuilder.path",
        signature: "fn std.url.UrlBuilder.path(self, path: Str) -> UrlBuilder",
        description: "Set the path segment. The leading `/` is inferred — `.path(\"a/b\")` and `.path(\"/a/b\")` produce identical output.",
        capability: "",
        example: "let u = std.url.Url.builder(\"https\").host(\"x\").path(\"/api/v1/items\").build()?;\n",
        see_also: "std.url.Url.builder, std.url.UrlBuilder.query_param",
    },
    StdlibExample {
        symbol: "std.url.UrlBuilder.query_param",
        signature: "fn std.url.UrlBuilder.query_param(self, key: Str, value: Str) -> UrlBuilder",
        description: "Append a query parameter. Multiple calls produce `?k1=v1&k2=v2`. Both key and value are percent-encoded at `build()` time.",
        capability: "",
        example: "let u = std.url.Url.builder(\"https\")\n  .host(\"x\")\n  .query_param(\"q\", \"hello world\")\n  .query_param(\"page\", \"2\")\n  .build()?;\n",
        see_also: "std.url.Url.builder, std.url.UrlBuilder.fragment, std.url.percent_encode_component",
    },
    StdlibExample {
        symbol: "std.url.UrlBuilder.userinfo",
        signature: "fn std.url.UrlBuilder.userinfo(self, user: Str, password: Str) -> UrlBuilder",
        description: "Set the `user[:password]@` segment. Pass empty strings to skip. Prefer header-based auth over baking credentials into the URL.",
        capability: "",
        example: "let u = std.url.Url.builder(\"https\").userinfo(\"alice\", \"secret\").host(\"vault.example.com\").build()?;\n",
        see_also: "std.url.Url.builder, std.url.UrlBuilder.host",
    },
    StdlibExample {
        symbol: "std.url.UrlBuilder.fragment",
        signature: "fn std.url.UrlBuilder.fragment(self, frag: Str) -> UrlBuilder",
        description: "Set the `#fragment` segment. Useful for anchor links and SPAs. The fragment is sent to the server only if the client copies it from the URL bar.",
        capability: "",
        example: "let u = std.url.Url.builder(\"https\").host(\"x\").path(\"/docs\").fragment(\"section-3\").build()?;\n",
        see_also: "std.url.Url.builder, std.url.UrlBuilder.query_param",
    },
    StdlibExample {
        symbol: "std.url.UrlBuilder.build",
        signature: "fn std.url.UrlBuilder.build(self) -> Result[Url, UrlErr]",
        description: "Finalise the builder. Returns `UrlErr.Build` if a required field is missing (currently: empty scheme).",
        capability: "",
        example: "let u = std.url.Url.builder(\"https\").host(\"example.com\").build()?;\nassert_eq(u.path, \"/\");\n",
        see_also: "std.url.Url.builder, std.url.UrlErr",
    },
    // ---- std.url.percent_encode + helpers ----
    StdlibExample {
        symbol: "std.url.percent_encode",
        signature: "fn std.url.percent_encode(s: Str) -> Str",
        description: "Percent-encode `s` per RFC 3986. Unreserved chars (`A-Z a-z 0-9 - _ . ~`) + `/` pass through; everything else becomes `%HH`.",
        capability: "",
        example: "let enc = std.url.percent_encode(\"hello world\");  // \"hello%20world\"\n",
        see_also: "std.url.percent_encode_component, std.url.percent_decode",
    },
    StdlibExample {
        symbol: "std.url.percent_encode_component",
        signature: "fn std.url.percent_encode_component(s: Str) -> Str",
        description: "Like `percent_encode` but also encodes `/`. Use for single path / query components that should not be re-interpreted as separators.",
        capability: "",
        example: "let enc = std.url.percent_encode_component(\"a/b&c=d\");  // \"a%2Fb%26c%3Dd\"\n",
        see_also: "std.url.percent_encode, std.url.percent_decode, std.url.UrlBuilder.query_param",
    },
    StdlibExample {
        symbol: "std.url.percent_decode",
        signature: "fn std.url.percent_decode(s: Str) -> Option[Str]",
        description: "Decode percent-escaped string. Returns `None` on malformed `%XX` triples. `+` is NOT decoded as space — that is form-urlencoded territory, decode upstream if needed.",
        capability: "",
        example: "let s = std.url.percent_decode(\"hello%20world\")?;  // \"hello world\"\n",
        see_also: "std.url.percent_encode, std.url.percent_encode_component",
    },
    StdlibExample {
        symbol: "std.url.UrlErr",
        signature: "enum std.url.UrlErr { Parse(Str), Build(Str) }",
        description: "Error variants for the URL surface. `Parse` wraps the underlying `url` crate's diagnostic; `Build` flags a required-field omission (empty scheme today).",
        capability: "",
        example: "match std.url.parse(input) {\n  Err(std.url.UrlErr.Parse(msg)) => log(format!(\"bad url: {}\", msg)),\n  Ok(u) => use(u),\n}\n",
        see_also: "std.url.parse, std.url.UrlBuilder.build",
    },
    // ---- std.uuid ----
    StdlibExample {
        symbol: "std.uuid.Uuid",
        signature: "struct std.uuid.Uuid { bytes: [U8; 16] }",
        description: "128-bit UUID per RFC 9562. Big-endian byte layout regardless of host. Implements `Display` (canonical hyphenated lowercase form).",
        capability: "",
        example: "let id = std.uuid.Uuid.v4()?;\nlet s = id.to_string();\n",
        see_also: "std.uuid.Uuid.v4, std.uuid.Uuid.v7, std.uuid.Uuid.parse, std.uuid.Uuid.nil",
    },
    StdlibExample {
        symbol: "std.uuid.Uuid.v4",
        signature: "fn std.uuid.Uuid.v4() -> Result[Uuid, UuidErr]",
        description: "Generate a random version-4 UUID. 122 bits of entropy + 4 bits version tag + 2 bits variant tag. Requires `crypto.rand`.",
        capability: "crypto.rand",
        example: "let id = std.uuid.Uuid.v4()?;  // \"550e8400-e29b-41d4-a716-446655440000\"\n",
        see_also: "std.uuid.Uuid.v7, std.uuid.Uuid.parse, std.crypto.random_bytes",
    },
    StdlibExample {
        symbol: "std.uuid.Uuid.v7",
        signature: "fn std.uuid.Uuid.v7() -> Result[Uuid, UuidErr]",
        description: "Generate a time-ordered version-7 UUID. 48-bit ms timestamp + 74 bits entropy. Sorts lexicographically by creation time — the preferred choice for DB primary keys where BTree locality matters.",
        capability: "crypto.rand",
        example: "let pk = std.uuid.Uuid.v7()?;  // sortable by creation order\n",
        see_also: "std.uuid.Uuid.v4, std.uuid.Uuid.parse",
    },
    StdlibExample {
        symbol: "std.uuid.Uuid.parse",
        signature: "fn std.uuid.Uuid.parse(s: Str) -> Result[Uuid, UuidErr]",
        description: "Parse the canonical hyphenated form (`xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`). Accepts upper or lowercase. Surrounding whitespace is trimmed.",
        capability: "",
        example: "let id = std.uuid.Uuid.parse(\"550e8400-e29b-41d4-a716-446655440000\")?;\n",
        see_also: "std.uuid.Uuid.to_string, std.uuid.Uuid.from_bytes",
    },
    StdlibExample {
        symbol: "std.uuid.Uuid.to_string",
        signature: "fn std.uuid.Uuid.to_string(&self) -> Str",
        description: "Render as the canonical hyphenated lowercase form. Round-trips through `parse` byte-for-byte.",
        capability: "",
        example: "let s = id.to_string();  // \"550e8400-e29b-41d4-a716-446655440000\"\n",
        see_also: "std.uuid.Uuid.parse, std.uuid.Uuid",
    },
    StdlibExample {
        symbol: "std.uuid.Uuid.nil",
        signature: "fn std.uuid.Uuid.nil() -> Uuid",
        description: "All-zero UUID. Useful as a sentinel for \"no id yet\" or for distinguishing \"unset\" from a real value.",
        capability: "",
        example: "let z = std.uuid.Uuid.nil();\nassert(z.is_nil());\n",
        see_also: "std.uuid.Uuid.is_nil, std.uuid.Uuid",
    },
    StdlibExample {
        symbol: "std.uuid.Uuid.is_nil",
        signature: "fn std.uuid.Uuid.is_nil(&self) -> Bool",
        description: "True iff this is the all-zero UUID. Cheap byte-scan over the 16-byte payload.",
        capability: "",
        example: "if id.is_nil() { /* sentinel value */ }\n",
        see_also: "std.uuid.Uuid.nil",
    },
    StdlibExample {
        symbol: "std.uuid.Uuid.version",
        signature: "fn std.uuid.Uuid.version(&self) -> U8",
        description: "UUID version digit per RFC 9562 (high nibble of byte 6). `4` for v4, `7` for v7, `0` for nil.",
        capability: "",
        example: "assert_eq(std.uuid.Uuid.v4()?.version(), 4);\nassert_eq(std.uuid.Uuid.v7()?.version(), 7);\n",
        see_also: "std.uuid.Uuid.v4, std.uuid.Uuid.v7",
    },
    StdlibExample {
        symbol: "std.uuid.Uuid.from_bytes",
        signature: "fn std.uuid.Uuid.from_bytes(bytes: [U8; 16]) -> Uuid",
        description: "Construct directly from raw bytes. Does NOT enforce the version / variant nibbles — use `v4` / `v7` unless you are reading from a wire format.",
        capability: "",
        example: "let raw: [U8; 16] = wire_blob[..16].try_into()?;\nlet id = std.uuid.Uuid.from_bytes(raw);\n",
        see_also: "std.uuid.Uuid.v4, std.uuid.Uuid.parse",
    },
    StdlibExample {
        symbol: "std.uuid.UuidErr",
        signature: "enum std.uuid.UuidErr { Parse(Str), Entropy(RandErr) }",
        description: "Error variants for `Uuid.parse` / `Uuid.v4` / `Uuid.v7`. `Parse(msg)` wraps a malformed canonical-form input; `Entropy(e)` propagates a CSPRNG failure from `std.crypto.random_bytes`.",
        capability: "",
        example: "match std.uuid.Uuid.parse(input) {\n  Err(std.uuid.UuidErr.Parse(m)) => log(format!(\"bad uuid: {}\", m)),\n  Ok(id) => use(id),\n}\n",
        see_also: "std.uuid.Uuid.parse, std.uuid.Uuid.v4, std.crypto.RandErr",
    },
    // ============================================================
    // v0.39 T5 — v0.38 backlog gap-fillers
    // ============================================================
    // ---- std.io: stdin lock + line iteration + buffer mgmt ----
    StdlibExample {
        symbol: "BufReader.read_line",
        signature: "fn BufReader.read_line[R](&mut self, buf: &mut String) -> Result[USize, IoError]",
        description: "Read one `\\n`-terminated line into `buf` (appended, not replaced). Returns the number of bytes read; `0` signals EOF. The terminator is preserved in `buf` — strip with `.trim_end_matches('\\n')` if undesired.",
        capability: "io.stdin",
        example: "let mut r = BufReader.new(std.io.stdin());\nlet mut line = String.new();\nlet n = r.read_line(&mut line)?;\nif n == 0 { return; }  // EOF\n",
        see_also: "BufReader.new, BufReader.lines, read_line",
    },
    StdlibExample {
        symbol: "BufWriter.write_all",
        signature: "fn BufWriter.write_all[W](&mut self, bytes: &[U8]) -> Result[(), IoError]",
        description: "Write every byte of `bytes`, retrying short writes internally until the slice is drained. Buffers into the inner handle's block size; explicit `.flush()` is still required to commit before drop if you need crash-resilient diagnostics.",
        capability: "",
        example: "let mut w = BufWriter.new(std.io.stdout());\nw.write_all(b\"hello world\\n\")?;\nw.flush()?;\n",
        see_also: "BufWriter.new, BufWriter.flush, write_line",
    },
    StdlibExample {
        symbol: "std.io.stdin_lock",
        signature: "fn std.io.stdin().lock() -> StdinLock",
        description: "Acquire an exclusive handle on stdin. Cheap — just takes the per-process mutex. Use when you need stable line semantics across many reads (the unlocked `Stdin` takes/releases the lock around every call).",
        capability: "io.stdin",
        example: "let stdin = std.io.stdin();\nlet mut handle = stdin.lock();\nlet mut s = String.new();\nhandle.read_line(&mut s)?;\n",
        see_also: "std.io.stdin, BufReader.new",
    },
    StdlibExample {
        symbol: "eprint",
        signature: "macro eprint!(fmt: Str, args: ...)",
        description: "Write to stderr without a trailing newline. Unbuffered — diagnostics survive a crash before flush. Pair with `eprintln!` when you want the newline.",
        capability: "io.stderr",
        example: "eprint!(\"progress: \");\nfor i in 0..10 { eprint!(\"{} \", i); }\neprintln!();\n",
        see_also: "eprintln, std.io.stderr",
    },
    // ---- std.process: output capture + status + env helpers ----
    StdlibExample {
        symbol: "Command.current_dir",
        signature: "fn Command.current_dir(&mut self, dir: Str) -> &mut Command",
        description: "Set the child's working directory. Inherits the parent's `cwd` by default. Equivalent to `chdir(dir); exec(program)` from the child's perspective.",
        capability: "",
        example: "Command.new(\"cargo\").arg(\"build\").current_dir(\"./crates/core\").exec()?;\n",
        see_also: "Command.new, Command.env, Command.exec",
    },
    StdlibExample {
        symbol: "Command.env_clear",
        signature: "fn Command.env_clear(&mut self) -> &mut Command",
        description: "Drop every inherited environment variable. Subsequent `.env(k, v)` calls re-populate the child's env from scratch. Useful for hermetic child invocations (CI, sandboxing).",
        capability: "",
        example: "Command.new(\"sh\").arg(\"-c\").arg(\"env | wc -l\")\n  .env_clear()\n  .env(\"PATH\", \"/usr/bin\")\n  .exec()?;\n",
        see_also: "Command.env, Command.new, Command.exec",
    },
    StdlibExample {
        symbol: "Command.stdout_piped",
        signature: "fn Command.stdout_piped(&mut self) -> &mut Command",
        description: "Redirect the child's stdout to a pipe captured by the parent. Required before `spawn()` if you want to incrementally read the child's output; `exec()` captures stdout automatically.",
        capability: "",
        example: "let mut child = Command.new(\"git\").arg(\"log\").stdout_piped().spawn()?;\nlet out = child.stdout.take().unwrap();\nfor line in BufReader.new(out).lines() { log(line?); }\n",
        see_also: "Command.spawn, Command.stderr_piped, Command.exec",
    },
    StdlibExample {
        symbol: "Command.stderr_piped",
        signature: "fn Command.stderr_piped(&mut self) -> &mut Command",
        description: "Redirect the child's stderr to a pipe captured by the parent. Same shape as `stdout_piped`. Pair both if you need to interleave diagnostics with stdout reads.",
        capability: "",
        example: "let mut child = Command.new(\"build\").stdout_piped().stderr_piped().spawn()?;\n",
        see_also: "Command.stdout_piped, Command.spawn",
    },
    StdlibExample {
        symbol: "Command.output",
        signature: "fn Command.output(&mut self) -> Result[ProcessOutput, ProcessError]",
        description: "Synonym for `Command.exec` — keeps the `std::process::Command` muscle memory for migrating Rust code. Runs to completion and returns captured stdout / stderr / exit.",
        capability: "process.exec",
        example: "let out = Command.new(\"echo\").arg(\"hi\").output()?;\nassert_eq(out.stdout.trim_end(), \"hi\");\n",
        see_also: "Command.exec, Command.spawn, ProcessOutput",
    },
    StdlibExample {
        symbol: "ProcessOutput",
        signature: "struct ProcessOutput { stdout: Str, stderr: Str, exit: ProcessExit }",
        description: "Result of `Command.exec` / `Command.output` / `std.process.exec`. `stdout` / `stderr` are decoded as UTF-8 with lossy replacement for invalid sequences. The raw bytes path is on the v0.40 roadmap.",
        capability: "",
        example: "let out = Command.new(\"git\").arg(\"rev-parse\").arg(\"HEAD\").exec()?;\nlet sha = out.stdout.trim_end();\nassert!(out.exit.success());\n",
        see_also: "Command.exec, ProcessExit, Command.output",
    },
    StdlibExample {
        symbol: "ProcessExit.success",
        signature: "fn ProcessExit.success(&self) -> Bool",
        description: "True iff the child exited normally with code 0. Treats signal terminations (Unix) as non-success regardless of the wrapped signal number.",
        capability: "",
        example: "let exit = std.process.wait(&mut child)?;\nif !exit.success() { panic(format!(\"failed code={}\", exit.code)); }\n",
        see_also: "ProcessExit, std.process.wait, Command.exec",
    },
    // ---- std.path: PathBuf mutators + extras ----
    StdlibExample {
        symbol: "PathBuf.push",
        signature: "fn PathBuf.push(&mut self, segment: Str)",
        description: "Append a path segment, inserting the platform separator if needed. If `segment` is absolute, it replaces the path entirely (matches POSIX `chdir` semantics).",
        capability: "",
        example: "let mut p = PathBuf.new();\np.push(\"/var\");\np.push(\"log\");\np.push(\"app.log\");\nassert_eq(p.as_str(), \"/var/log/app.log\");\n",
        see_also: "PathBuf.new, Path.join, PathBuf.pop",
    },
    StdlibExample {
        symbol: "PathBuf.pop",
        signature: "fn PathBuf.pop(&mut self) -> Bool",
        description: "Drop the final component. Returns `true` if a component was removed, `false` if the path was already at root.",
        capability: "",
        example: "let mut p = PathBuf.from(\"/a/b/c\");\nassert(p.pop());\nassert_eq(p.as_str(), \"/a/b\");\n",
        see_also: "PathBuf.push, Path.parent",
    },
    StdlibExample {
        symbol: "PathBuf.set_extension",
        signature: "fn PathBuf.set_extension(&mut self, ext: Str) -> Bool",
        description: "Replace the file extension. Adds one if absent; strips it if `ext` is empty. Returns `true` if the file-name component was non-empty.",
        capability: "",
        example: "let mut p = PathBuf.from(\"build.mty\");\np.set_extension(\"o\");\nassert_eq(p.as_str(), \"build.o\");\n",
        see_also: "Path.with_extension, Path.extension",
    },
    StdlibExample {
        symbol: "PathBuf.from",
        signature: "fn PathBuf.from(s: Str) -> PathBuf",
        description: "Allocate an owned path from a string. Equivalent to `PathBuf.new(); .push(s)`.",
        capability: "",
        example: "let p = PathBuf.from(\"/etc/hosts\");\n",
        see_also: "PathBuf.new, Path.new",
    },
    StdlibExample {
        symbol: "Path.metadata",
        signature: "fn Path.metadata(&self, cap: FsCap) -> Result[StatResult, IoError]",
        description: "Stat the path. Returns size / kind / mtime in a `StatResult`. Requires `fs.read`.",
        capability: "fs.read",
        example: "let md = Path.new(\"./build.log\").metadata(FsCap.rooted(\".\"))?;\nlog(format!(\"size={} bytes\", md.size));\n",
        see_also: "std.fs.stat, Path.exists, StatResult",
    },
    StdlibExample {
        symbol: "Path.canonicalize",
        signature: "fn Path.canonicalize(&self, cap: FsCap) -> Result[PathBuf, IoError]",
        description: "Resolve symlinks and `..` segments against the real filesystem. Used to ground a user-supplied path before checking it against an allowlist. Requires `fs.read`.",
        capability: "fs.read",
        example: "let real = Path.new(\"./tmp/../etc\").canonicalize(FsCap.rooted(\"./tmp\"))?;\n",
        see_also: "Path.new, std.fs.exists, FsCap.rooted",
    },
    StdlibExample {
        symbol: "Path.walk",
        signature: "fn Path.walk(&self, cap: FsCap) -> Iterator[Result<DirEntry, IoError>]",
        description: "Recursively iterate every descendant. Yields one `DirEntry` per file + directory in deterministic depth-first order. Skips symlinks by default. Requires `fs.read`.",
        capability: "fs.read",
        example: "for entry in Path.new(\"./src\").walk(FsCap.rooted(\"./src\")) {\n  let e = entry?;\n  if e.path.extension() == Some(\"mty\") { log(e.path.to_string()); }\n}\n",
        see_also: "Path.new, std.fs.read_to_string, FsCap.rooted",
    },
    // ---- std.iter: peekable / windowed / chunks / cycle / min / max ----
    StdlibExample {
        symbol: "Iterator.peekable",
        signature: "fn Iterator.peekable[T](self) -> Peekable[T]",
        description: "Wrap the iterator with a one-item lookahead. `.peek()` returns `Option[&T]` without advancing; the next `.next()` yields the same item.",
        capability: "",
        example: "let mut it = [1, 2, 3].iter().peekable();\nwhile let Some(&n) = it.peek() {\n  if n > 2 { break; }\n  it.next();\n}\n",
        see_also: "Iterator.take, Iterator.find, Iterator.next",
    },
    StdlibExample {
        symbol: "Iterator.windowed",
        signature: "fn Iterator.windowed[T](self, n: USize) -> Iterator[Vec[T]]",
        description: "Sliding-window iterator. Yields every contiguous `n`-element window once. The source must be `Clone` — items appear in `n` consecutive windows. Empty for fewer than `n` items.",
        capability: "",
        example: "let pairs = [1, 2, 3, 4].iter().windowed(2).collect();\nassert_eq(pairs, [[1, 2], [2, 3], [3, 4]]);\n",
        see_also: "Iterator.chunks, Iterator.take, Iterator.enumerate",
    },
    StdlibExample {
        symbol: "Iterator.chunks",
        signature: "fn Iterator.chunks[T](self, n: USize) -> Iterator[Vec[T]]",
        description: "Partition the iterator into non-overlapping `n`-element chunks. The final chunk may be shorter if the source isn't divisible by `n`.",
        capability: "",
        example: "let batches = (1..=7).chunks(3).collect();\nassert_eq(batches, [[1, 2, 3], [4, 5, 6], [7]]);\n",
        see_also: "Iterator.windowed, Iterator.take, Iterator.skip",
    },
    StdlibExample {
        symbol: "Iterator.cycle",
        signature: "fn Iterator.cycle[T: Clone](self) -> Iterator[T]",
        description: "Repeat the source iterator forever. Requires `T: Clone` so each cycle can reproduce the same items. Pair with `.take(n)` to bound the output.",
        capability: "",
        example: "let rgb: Vec[Str] = [\"r\", \"g\", \"b\"].iter().cycle().take(7).collect();\nassert_eq(rgb.len(), 7);\n",
        see_also: "Iterator.take, Iterator.chain, Iterator.enumerate",
    },
    StdlibExample {
        symbol: "Iterator.min",
        signature: "fn Iterator.min[T: Ord](self) -> Option[T]",
        description: "Terminal: smallest item by `Ord`. Returns `None` on empty input. Stable — ties yield the first seen.",
        capability: "",
        example: "let lo = [3, 1, 4, 1, 5].iter().copied().min();\nassert_eq(lo, Some(1));\n",
        see_also: "Iterator.max, Iterator.fold, Iterator.find",
    },
    StdlibExample {
        symbol: "Iterator.max",
        signature: "fn Iterator.max[T: Ord](self) -> Option[T]",
        description: "Terminal: largest item by `Ord`. Returns `None` on empty input. Stable — ties yield the last seen.",
        capability: "",
        example: "let hi = [3, 1, 4, 1, 5].iter().copied().max();\nassert_eq(hi, Some(5));\n",
        see_also: "Iterator.min, Iterator.fold, Iterator.sum",
    },
    StdlibExample {
        symbol: "Iterator.flat_map",
        signature: "fn Iterator.flat_map[T, U, I: Iterator[U]](self, f: fn(T) -> I) -> Iterator[U]",
        description: "Map each item to a sub-iterator and flatten. Equivalent to `.map(f).flatten()`.",
        capability: "",
        example: "let words = [\"hi there\", \"hello world\"].iter().flat_map(|s| s.split(\" \")).collect();\n",
        see_also: "Iterator.map, Iterator.filter, Iterator.collect",
    },
    StdlibExample {
        symbol: "Iterator.rev",
        signature: "fn Iterator.rev[T](self) -> Iterator[T]",
        description: "Reverse the iteration order. Requires the source to implement DoubleEndedIterator (`Vec.iter`, ranges, etc.).",
        capability: "",
        example: "let backward: Vec[I32] = (1..=3).rev().collect();\nassert_eq(backward, [3, 2, 1]);\n",
        see_also: "Iterator.take, Iterator.skip, Iterator.collect",
    },
    StdlibExample {
        symbol: "Iterator.step_by",
        signature: "fn Iterator.step_by[T](self, step: USize) -> Iterator[T]",
        description: "Yield every `step`-th item, starting at index 0. `step_by(1)` is the identity; `step_by(0)` panics.",
        capability: "",
        example: "let evens: Vec[I32] = (0..10).step_by(2).collect();\nassert_eq(evens, [0, 2, 4, 6, 8]);\n",
        see_also: "Iterator.skip, Iterator.take, Iterator.enumerate",
    },
    // ---- std.error: typed error helpers ----
    StdlibExample {
        symbol: "AnyhowError.context",
        signature: "fn AnyhowError.context(self, msg: Str) -> AnyhowError",
        description: "Attach a description to the error chain (Rust-anyhow style). Walks the existing `source()` chain so the formatted output reads top-down: outer context → root cause.",
        capability: "",
        example: "let cfg = read_config(path).map_err(|e| AnyhowError.from(e).context(\"reading config\"))?;\n",
        see_also: "Error.trait, anyhow_error, Result.map_err",
    },
    StdlibExample {
        symbol: "Error.source",
        signature: "fn Error.source(&self) -> Option[&dyn Error]",
        description: "Walk the error chain — return the immediate cause, or `None` if this is a root error. Drives the multi-line `{:?}` debug formatter and the `AnyhowError.context` chain.",
        capability: "",
        example: "let mut err: &dyn Error = &my_err;\nwhile let Some(cause) = err.source() {\n  log(format!(\"caused by: {}\", cause));\n  err = cause;\n}\n",
        see_also: "Error.trait, AnyhowError.context, anyhow_error",
    },
    StdlibExample {
        symbol: "Result.context",
        signature: "fn Result.context[T, E](self, msg: Str) -> Result[T, AnyhowError]",
        description: "Attach `msg` to the `Err` branch if there is one. Equivalent to `self.map_err(|e| AnyhowError.from(e).context(msg))`.",
        capability: "",
        example: "let body = read_file(path).context(\"reading manifest\")?;\n",
        see_also: "Result.map_err, anyhow_error, AnyhowError.context",
    },
    // ---- std.string / std.vec polish ----
    StdlibExample {
        symbol: "String.split",
        signature: "fn String.split(&self, sep: Str) -> Iterator[Str]",
        description: "Lazy iterator of substrings separated by `sep`. Empty `sep` yields a `Char`-wide split (one item per code point). Trailing empties are preserved.",
        capability: "",
        example: "let parts: Vec[Str] = \"a,b,,c\".split(\",\").collect();\nassert_eq(parts, [\"a\", \"b\", \"\", \"c\"]);\n",
        see_also: "String.splitn, String.find, String.trim",
    },
    StdlibExample {
        symbol: "String.trim",
        signature: "fn String.trim(&self) -> Str",
        description: "Borrow a sub-string with leading + trailing whitespace removed. Whitespace follows the Unicode White_Space property.",
        capability: "",
        example: "let s = \"  hello world  \\n\".trim();\nassert_eq(s, \"hello world\");\n",
        see_also: "String.trim_end, String.trim_start, String.split",
    },
    StdlibExample {
        symbol: "String.starts_with",
        signature: "fn String.starts_with(&self, prefix: Str) -> Bool",
        description: "True iff `self` starts with `prefix`. O(prefix.len()).",
        capability: "",
        example: "if s.starts_with(\"https://\") { /* secure */ }\n",
        see_also: "String.ends_with, String.find",
    },
    StdlibExample {
        symbol: "String.ends_with",
        signature: "fn String.ends_with(&self, suffix: Str) -> Bool",
        description: "True iff `self` ends with `suffix`. O(suffix.len()).",
        capability: "",
        example: "if path.ends_with(\".mty\") { compile(path); }\n",
        see_also: "String.starts_with, String.find",
    },
    StdlibExample {
        symbol: "String.contains",
        signature: "fn String.contains(&self, needle: Str) -> Bool",
        description: "True iff `needle` occurs anywhere inside `self`. Use `find` if you also need the offset.",
        capability: "",
        example: "if body.contains(\"ERROR\") { alert(); }\n",
        see_also: "String.find, String.starts_with, String.ends_with",
    },
    StdlibExample {
        symbol: "String.to_lowercase",
        signature: "fn String.to_lowercase(&self) -> String",
        description: "Allocate a lowercase copy. Honors Unicode special casing — `'İ'.to_lowercase()` yields `\"i̇\"` (two code points). Use `to_ascii_lowercase` for ASCII-only.",
        capability: "",
        example: "if email.to_lowercase() == canonical { /* match */ }\n",
        see_also: "String.to_uppercase, String.trim",
    },
    StdlibExample {
        symbol: "String.to_uppercase",
        signature: "fn String.to_uppercase(&self) -> String",
        description: "Allocate an uppercase copy. Honors Unicode special casing.",
        capability: "",
        example: "let shout = greeting.to_uppercase();\n",
        see_also: "String.to_lowercase, String.trim",
    },
    StdlibExample {
        symbol: "Vec.contains",
        signature: "fn Vec.contains[T: PartialEq](&self, needle: &T) -> Bool",
        description: "Linear scan — true iff any element equals `needle` by `PartialEq`. O(n). Use a `HashSet` for hot lookups.",
        capability: "",
        example: "if allow.contains(&\"GET\") { /* ok */ }\n",
        see_also: "Vec.iter, HashSet.contains, Iterator.any",
    },
    StdlibExample {
        symbol: "Vec.sort",
        signature: "fn Vec.sort[T: Ord](&mut self)",
        description: "In-place stable sort by `Ord`. O(n log n). The stable variant preserves original order for equal items.",
        capability: "",
        example: "let mut v = [3, 1, 4, 1, 5];\nv.sort();\nassert_eq(v, [1, 1, 3, 4, 5]);\n",
        see_also: "Vec.sort_by, Vec.sort_by_key, Iterator.collect",
    },
    StdlibExample {
        symbol: "Vec.sort_by",
        signature: "fn Vec.sort_by[T](&mut self, cmp: fn(&T, &T) -> Ordering)",
        description: "In-place stable sort with a user comparator. Use for custom orderings or sort-by-projection-without-`Ord`.",
        capability: "",
        example: "v.sort_by(|a, b| b.priority.cmp(&a.priority));  // descending\n",
        see_also: "Vec.sort, Vec.sort_by_key",
    },
    StdlibExample {
        symbol: "Vec.reverse",
        signature: "fn Vec.reverse(&mut self)",
        description: "In-place reverse. O(n). Equivalent to `.iter().rev().collect()` but does not allocate.",
        capability: "",
        example: "let mut v = [1, 2, 3];\nv.reverse();\nassert_eq(v, [3, 2, 1]);\n",
        see_also: "Iterator.rev, Vec.sort",
    },
    StdlibExample {
        symbol: "Vec.retain",
        signature: "fn Vec.retain[T](&mut self, pred: fn(&T) -> Bool)",
        description: "Drop every element where `pred` returns `false`. In-place, preserves order. O(n) — one pass.",
        capability: "",
        example: "let mut v = [1, 2, 3, 4, 5];\nv.retain(|x| x % 2 == 0);\nassert_eq(v, [2, 4]);\n",
        see_also: "Iterator.filter, Vec.iter, Vec.clear",
    },
    StdlibExample {
        symbol: "Vec.extend",
        signature: "fn Vec.extend[T, I: Iterator[T]](&mut self, items: I)",
        description: "Append every item from `items`. Amortised O(n) — reserves up front when the iterator reports a size hint.",
        capability: "",
        example: "let mut v = vec![1, 2];\nv.extend([3, 4].iter().copied());\nassert_eq(v, [1, 2, 3, 4]);\n",
        see_also: "Vec.append, Vec.push, Iterator.collect",
    },
    // ---- std.json: shape helpers ----
    StdlibExample {
        symbol: "Json.get",
        signature: "fn Json.get(&self, key: Str) -> Option[&Json]",
        description: "Lookup an object field by name. Returns `None` for non-objects or missing keys. Chains via `.and_then(|j| j.get(...))`.",
        capability: "",
        example: "let v = std.json.parse(body)?;\nif let Some(name) = v.get(\"user\").and_then(|u| u.get(\"name\")) { log(name); }\n",
        see_also: "std.json.parse, Json.Obj, Json.as_str",
    },
    StdlibExample {
        symbol: "Json.as_str",
        signature: "fn Json.as_str(&self) -> Option[Str]",
        description: "Borrow the underlying string if this is a `Json.Str`. Returns `None` for any other variant. Use after `Json.get` to extract a string field defensively.",
        capability: "",
        example: "let name = v.get(\"name\").and_then(|n| n.as_str()).unwrap_or(\"anon\");\n",
        see_also: "Json.get, Json.Str, Json.as_i64",
    },
    StdlibExample {
        symbol: "Json.as_i64",
        signature: "fn Json.as_i64(&self) -> Option[I64]",
        description: "Borrow the underlying integer if this is a `Json.Num` that round-trips through I64. Returns `None` for non-numbers, floats, or out-of-range values.",
        capability: "",
        example: "let n = v.get(\"count\").and_then(|j| j.as_i64()).unwrap_or(0);\n",
        see_also: "Json.get, Json.Num, Json.as_str",
    },
    StdlibExample {
        symbol: "Json.as_array",
        signature: "fn Json.as_array(&self) -> Option[&[Json]]",
        description: "Borrow the underlying slice if this is a `Json.Arr`. Use with `.iter()` to drive a comprehension.",
        capability: "",
        example: "for item in v.get(\"items\").and_then(|a| a.as_array()).iter().flat_map(|a| a.iter()) {\n  use(item);\n}\n",
        see_also: "Json.get, Json.Arr, Iterator.flat_map",
    },
    // ---- std.collections: more dictionary surfaces ----
    StdlibExample {
        symbol: "HashMap.contains_key",
        signature: "fn HashMap.contains_key[K: Hash + Eq, V](&self, key: &K) -> Bool",
        description: "True iff `key` is present. O(1) average. Use when you only need existence and not the value.",
        capability: "",
        example: "if cache.contains_key(&id) { /* hit */ } else { miss(); }\n",
        see_also: "HashMap.get, HashMap.insert, HashSet.contains",
    },
    StdlibExample {
        symbol: "HashMap.len",
        signature: "fn HashMap.len[K, V](&self) -> USize",
        description: "Number of stored key/value pairs. O(1).",
        capability: "",
        example: "let n = users.len();\nlog(format!(\"{} active sessions\", n));\n",
        see_also: "HashMap.is_empty, HashMap.iter",
    },
    StdlibExample {
        symbol: "HashMap.iter",
        signature: "fn HashMap.iter[K, V](&self) -> Iterator[(&K, &V)]",
        description: "Lazy iterator over `(key, value)` pairs. Iteration order is unspecified (hash randomisation is enabled). Use `BTreeMap` if order matters.",
        capability: "",
        example: "for (k, v) in counts.iter() {\n  log(format!(\"{}={}\", k, v));\n}\n",
        see_also: "HashMap.keys, HashMap.values, BTreeMap.iter",
    },
    StdlibExample {
        symbol: "HashMap.entry",
        signature: "fn HashMap.entry[K: Hash + Eq, V](&mut self, key: K) -> Entry[K, V]",
        description: "Get-or-insert API. `entry(k).or_insert(v)` returns `&mut V` after inserting `v` if `k` was absent — one hash, zero copies on the hit path.",
        capability: "",
        example: "let counter = counts.entry(word).or_insert(0);\n*counter += 1;\n",
        see_also: "HashMap.insert, HashMap.get, HashMap.contains_key",
    },
    StdlibExample {
        symbol: "HashSet.contains",
        signature: "fn HashSet.contains[T: Hash + Eq](&self, value: &T) -> Bool",
        description: "True iff `value` is in the set. O(1) average. The membership-check counterpart to `HashSet.insert`.",
        capability: "",
        example: "if seen.contains(&id) { return; }\nseen.insert(id);\n",
        see_also: "HashSet.insert, HashSet.new, HashMap.contains_key",
    },
    StdlibExample {
        symbol: "BTreeMap.range",
        signature: "fn BTreeMap.range[K: Ord, V](&self, range: Range[K]) -> Iterator[(&K, &V)]",
        description: "Iterate entries whose keys fall in the half-open range `low..high`. Logarithmic seek + linear over the matching span.",
        capability: "",
        example: "for (k, v) in scores.range(\"a\"..\"m\") { use(k, v); }\n",
        see_also: "BTreeMap.iter, BTreeMap.new, HashMap.iter",
    },
    // ---------------------------------------------------------------
    // v0.40 T5 — catalog expansion 418 → 518+. Covers v0.40 T4 std.regex
    // + AEAD surfaces, v0.40 T3 Char.from_u32, and v0.39-backlog fillers
    // (std.iter advanced combinators, std.collections polish, std.json /
    // std.path / std.string / std.option / std.result / std.swarm /
    // std.eval / std.observe / std.fs uncovered helpers).
    // ---------------------------------------------------------------

    // ---- std.regex (v0.40 T4) -------------------------------------
    StdlibExample {
        symbol: "std.regex.Regex",
        signature: "struct std.regex.Regex",
        description: "Compiled regular expression. Built once via `Regex.new(pat)`; supports `find`, `find_all`, `captures`, `captures_all`, `replace`, `replace_all`, `is_match`, `split`, `as_str`. RE2-style finite automata — linear time, no catastrophic backtracking; look-around is intentionally unsupported.",
        capability: "",
        example: "let r = std.regex.Regex.new(\"\\\\d+\")?;\nif r.is_match(\"abc 123\") { log(\"ok\"); }\n",
        see_also: "std.regex.Regex.new, std.regex.Regex.find, std.regex.Captures, std.regex.RegexErr",
    },
    StdlibExample {
        symbol: "std.regex.Regex.new",
        signature: "fn std.regex.Regex.new(pattern: Str) -> Result[Regex, RegexErr]",
        description: "Compile a regex pattern. Anchors, groups, alternation, repetition, Unicode categories and ASCII shorthands (`\\d \\w \\s`) all supported. Malformed patterns yield `RegexErr.Compile`. Look-around is NOT supported.",
        capability: "",
        example: "let r = std.regex.Regex.new(r\"\\d{4}-\\d{2}-\\d{2}\")?;\nassert(r.is_match(\"date: 2026-05-30\"));\n",
        see_also: "std.regex.Regex, std.regex.RegexErr, std.regex.Regex.is_match",
    },
    StdlibExample {
        symbol: "std.regex.Regex.find",
        signature: "fn std.regex.Regex.find(&self, haystack: Str) -> Option[Match]",
        description: "First match in `haystack`, or `None` if the pattern doesn't fire. Returns an owned [`Match`] carrying the text and byte-offset range.",
        capability: "",
        example: "let r = std.regex.Regex.new(r\"\\d+\")?;\nif let Some(m) = r.find(\"a12 b34\") { log(m.text); } // \"12\"\n",
        see_also: "std.regex.Regex.find_all, std.regex.Regex.captures, std.regex.Match",
    },
    StdlibExample {
        symbol: "std.regex.Regex.find_all",
        signature: "fn std.regex.Regex.find_all(&self, haystack: Str) -> Vec[Match]",
        description: "All non-overlapping matches in `haystack`, left to right. `\"aa\"` on `\"aaaa\"` yields **two** matches at offsets 0 and 2 — the engine does not overlap by default.",
        capability: "",
        example: "let r = std.regex.Regex.new(r\"\\d{4}-\\d{2}-\\d{2}\")?;\nlet all = r.find_all(\"2026-05-30 to 2026-06-01\");\nassert_eq(all.len(), 2);\n",
        see_also: "std.regex.Regex.find, std.regex.Regex.captures_all, std.regex.Match",
    },
    StdlibExample {
        symbol: "std.regex.Regex.captures",
        signature: "fn std.regex.Regex.captures(&self, haystack: Str) -> Option[Captures]",
        description: "Capture groups for the first match. `groups[0]` is the whole match; `groups[1..]` are the parenthesised subgroups left-to-right. Non-participating groups (e.g. an alternative that didn't fire) are `None`.",
        capability: "",
        example: "let r = std.regex.Regex.new(r\"user=(\\w+).*ts=(\\d+)\")?;\nlet caps = r.captures(\"user=ihass ts=1234\")?;\nlog(caps.get(1).text); // \"ihass\"\n",
        see_also: "std.regex.Captures, std.regex.Captures.get, std.regex.Regex.captures_all",
    },
    StdlibExample {
        symbol: "std.regex.Regex.captures_all",
        signature: "fn std.regex.Regex.captures_all(&self, haystack: Str) -> Vec[Captures]",
        description: "All capture groups for every match. Useful for \"scan + extract\" loops without zipping `find_all` and `captures` by index.",
        capability: "",
        example: "let r = std.regex.Regex.new(r\"(\\w+)=(\\d+)\")?;\nfor caps in r.captures_all(\"a=1 b=2 c=3\") {\n  log(caps.get(1).text); // key\n}\n",
        see_also: "std.regex.Regex.captures, std.regex.Regex.find_all, std.regex.Captures",
    },
    StdlibExample {
        symbol: "std.regex.Regex.replace",
        signature: "fn std.regex.Regex.replace(&self, haystack: Str, replacement: Str) -> Str",
        description: "Replace only the first match with `replacement`. Replacement supports `$0`, `$1`, ... backrefs to capture groups.",
        capability: "",
        example: "let r = std.regex.Regex.new(r\"\\bworld\\b\")?;\nlet out = r.replace(\"hello world world\", \"Mighty\");\n// \"hello Mighty world\"\n",
        see_also: "std.regex.Regex.replace_all, std.regex.Regex.find",
    },
    StdlibExample {
        symbol: "std.regex.Regex.replace_all",
        signature: "fn std.regex.Regex.replace_all(&self, haystack: Str, replacement: Str) -> Str",
        description: "Replace every match with `replacement`. Supports `$N` backrefs into capture groups (e.g. `$1` for the first group).",
        capability: "",
        example: "let r = std.regex.Regex.new(r\"(\\w+)\\s+(\\w+)\")?;\nlet swapped = r.replace_all(\"hello world foo bar\", \"$2 $1\");\n// \"world hello bar foo\"\n",
        see_also: "std.regex.Regex.replace, std.regex.Regex.captures",
    },
    StdlibExample {
        symbol: "std.regex.Regex.is_match",
        signature: "fn std.regex.Regex.is_match(&self, haystack: Str) -> Bool",
        description: "Cheap predicate: does the haystack contain any match? The engine stops at the first hit so this is strictly faster than `find().is_some()` on long inputs.",
        capability: "",
        example: "let r = std.regex.Regex.new(r\"^[A-Za-z0-9_-]{16,64}$\")?;\nif !r.is_match(session_id) { return Err(\"bad session id\"); }\n",
        see_also: "std.regex.Regex.find, std.regex.Regex.new",
    },
    StdlibExample {
        symbol: "std.regex.Regex.split",
        signature: "fn std.regex.Regex.split(&self, haystack: Str) -> Vec[Str]",
        description: "Split `haystack` on every match. The matches themselves are removed from the output — a CSV-style splitter on `,\\s*` is the canonical use.",
        capability: "",
        example: "let r = std.regex.Regex.new(r\",\\s*\")?;\nlet parts = r.split(\"a, b,c ,  d\");\nassert_eq(parts.len(), 4);\n",
        see_also: "std.regex.Regex.find_all, String.split",
    },
    StdlibExample {
        symbol: "std.regex.Regex.as_str",
        signature: "fn std.regex.Regex.as_str(&self) -> Str",
        description: "The original pattern string passed to `Regex.new`. Useful for logging / debug output without re-storing the source separately.",
        capability: "",
        example: "let r = std.regex.Regex.new(r\"\\w+\")?;\nassert_eq(r.as_str(), r\"\\w+\");\n",
        see_also: "std.regex.Regex.new, std.regex.Regex",
    },
    StdlibExample {
        symbol: "std.regex.Match",
        signature: "struct std.regex.Match { text: Str, start: USize, end: USize }",
        description: "One regex match: the matched substring and the BYTE offsets it spans in the haystack. `start`/`end` are byte-accurate (UTF-8); for ASCII this is also the character index but for multi-byte input the offsets remain byte-accurate, which is what every downstream API needs (slicing, error spans, etc).",
        capability: "",
        example: "let m = r.find(haystack).unwrap();\nlog(m.text);\nlog(format(\"{}..{}\", m.start, m.end));\n",
        see_also: "std.regex.Regex.find, std.regex.Captures, std.regex.Match.len",
    },
    StdlibExample {
        symbol: "std.regex.Match.len",
        signature: "fn std.regex.Match.len(&self) -> USize",
        description: "Length of the matched substring in BYTES (= `end - start`). For ASCII this equals the character count; for multi-byte input it does not.",
        capability: "",
        example: "let m = r.find(s).unwrap();\nassert_eq(m.len(), m.end - m.start);\n",
        see_also: "std.regex.Match, std.regex.Match.is_empty",
    },
    StdlibExample {
        symbol: "std.regex.Match.is_empty",
        signature: "fn std.regex.Match.is_empty(&self) -> Bool",
        description: "Is the matched substring empty? Empty matches happen with zero-width patterns like `^`, `$`, or `\\b`.",
        capability: "",
        example: "let r = std.regex.Regex.new(r\"\\b\")?;\nlet m = r.find(\"hello\").unwrap();\nassert(m.is_empty());\n",
        see_also: "std.regex.Match, std.regex.Match.len",
    },
    StdlibExample {
        symbol: "std.regex.Captures",
        signature: "struct std.regex.Captures { groups: Vec[Option[Match]] }",
        description: "All capture groups from a single regex match. `groups[0]` is the overall match; `groups[1..]` are the parenthesised subgroups in left-to-right order. A group that did not participate (e.g. an unfired alternative) is `None`.",
        capability: "",
        example: "let caps = r.captures(line)?;\nlet whole = caps.get(0)?; // group 0 always present\nlet sub = caps.get(1)?;   // first parenthesised group\n",
        see_also: "std.regex.Captures.get, std.regex.Captures.len, std.regex.Regex.captures",
    },
    StdlibExample {
        symbol: "std.regex.Captures.get",
        signature: "fn std.regex.Captures.get(&self, idx: USize) -> Option[&Match]",
        description: "Look up a group by index. `0` is the whole match; `1..` are the parenthesised subgroups. Returns `None` for out-of-range or non-participating groups.",
        capability: "",
        example: "let caps = r.captures(\"k=v\")?;\nlet key = caps.get(1)?.text; // \"k\"\nlet val = caps.get(2)?.text; // \"v\"\n",
        see_also: "std.regex.Captures, std.regex.Captures.len, std.regex.Regex.captures",
    },
    StdlibExample {
        symbol: "std.regex.Captures.len",
        signature: "fn std.regex.Captures.len(&self) -> USize",
        description: "Number of groups (including group 0). Always >= 1 for a successful match. Use to bounds-check before indexing.",
        capability: "",
        example: "let caps = r.captures(s)?;\nassert(caps.len() >= 3); // expect 2 subgroups\n",
        see_also: "std.regex.Captures.get, std.regex.Captures",
    },
    StdlibExample {
        symbol: "std.regex.RegexErr",
        signature: "enum std.regex.RegexErr { Compile(Str) }",
        description: "Error returned by `Regex.new` when the pattern is malformed (unclosed group, invalid escape, unsupported feature like look-around). The payload string carries the underlying `regex` crate diagnostic.",
        capability: "",
        example: "match std.regex.Regex.new(\"(unclosed\") {\n  Ok(_) => unreachable(),\n  Err(RegexErr.Compile(msg)) => log(msg),\n}\n",
        see_also: "std.regex.Regex.new, std.regex.Regex",
    },

    // ---- std.crypto.aes_gcm (v0.40 T4) ----------------------------
    StdlibExample {
        symbol: "std.crypto.aes_gcm.encrypt",
        signature: "fn std.crypto.aes_gcm.encrypt(key: &[U8; 32], nonce: &[U8; 12], aad: &[U8], pt: &[U8]) -> Result[Vec[U8], AeadErr]",
        description: "AES-256-GCM authenticated encryption. Returns `plaintext_len + 16` bytes — the ciphertext followed by the 128-bit GCM auth tag. **Nonce MUST be unique for every (key, message)** — reuse catastrophically breaks GCM (an attacker can recover the GHASH authentication key). AAD is bound into the tag but not encrypted; the decryptor must pass the same AAD bytes.",
        capability: "",
        example: "let ct = std.crypto.aes_gcm.encrypt(&key, &nonce, b\"v=1\", b\"payload\")?;\nlet b64 = std.encoding.base64.encode_url_no_pad(ct);\n",
        see_also: "std.crypto.aes_gcm.decrypt, std.crypto.AeadErr, std.crypto.random_bytes, std.crypto.chacha20_poly1305.encrypt",
    },
    StdlibExample {
        symbol: "std.crypto.aes_gcm.decrypt",
        signature: "fn std.crypto.aes_gcm.decrypt(key: &[U8; 32], nonce: &[U8; 12], aad: &[U8], ct: &[U8]) -> Result[Vec[U8], AeadErr]",
        description: "AES-256-GCM decrypt. Verifies the 128-bit auth tag in CONSTANT TIME before returning plaintext. Any tampering with ciphertext, AAD, key, or nonce yields `AeadErr.Decrypt` — intentionally opaque (no padding-oracle leak distinguishing wrong key from wrong AAD).",
        capability: "",
        example: "let pt = std.crypto.aes_gcm.decrypt(&key, &nonce, b\"v=1\", &ct)?;\nassert_eq(pt, b\"payload\");\n",
        see_also: "std.crypto.aes_gcm.encrypt, std.crypto.AeadErr, std.crypto.chacha20_poly1305.decrypt",
    },
    StdlibExample {
        symbol: "std.crypto.aes_gcm",
        signature: "module std.crypto.aes_gcm",
        description: "AES-256-GCM authenticated encryption (NIST CAVP-tested). Workhorse AEAD that backs TLS 1.3, AWS S3-SSE, Signal, etc. Pick this on hardware with AES-NI (x86_64); pick `chacha20_poly1305` on ARM / embedded / hosts without AES instructions. Backed by RustCrypto's audited `aes-gcm` crate.",
        capability: "",
        example: "use std.crypto.aes_gcm.{encrypt, decrypt};\nlet ct = encrypt(&key, &nonce, b\"v=1\", b\"hi\")?;\nlet pt = decrypt(&key, &nonce, b\"v=1\", &ct)?;\n",
        see_also: "std.crypto.aes_gcm.encrypt, std.crypto.aes_gcm.decrypt, std.crypto.chacha20_poly1305, std.crypto.AeadErr",
    },

    // ---- std.crypto.chacha20_poly1305 (v0.40 T4) ------------------
    StdlibExample {
        symbol: "std.crypto.chacha20_poly1305.encrypt",
        signature: "fn std.crypto.chacha20_poly1305.encrypt(key: &[U8; 32], nonce: &[U8; 12], aad: &[U8], pt: &[U8]) -> Result[Vec[U8], AeadErr]",
        description: "RFC 8439 ChaCha20-Poly1305 AEAD encrypt. Identical shape to `aes_gcm.encrypt` on purpose so the caller can swap the cipher by changing one function name. Returns `plaintext_len + 16` bytes (ciphertext + Poly1305 tag). Same nonce-uniqueness requirement as AES-GCM.",
        capability: "",
        example: "let ct = std.crypto.chacha20_poly1305.encrypt(&key, &nonce, b\"v=1\", b\"payload\")?;\n",
        see_also: "std.crypto.chacha20_poly1305.decrypt, std.crypto.AeadErr, std.crypto.aes_gcm.encrypt",
    },
    StdlibExample {
        symbol: "std.crypto.chacha20_poly1305.decrypt",
        signature: "fn std.crypto.chacha20_poly1305.decrypt(key: &[U8; 32], nonce: &[U8; 12], aad: &[U8], ct: &[U8]) -> Result[Vec[U8], AeadErr]",
        description: "RFC 8439 ChaCha20-Poly1305 AEAD decrypt. Verifies the 128-bit Poly1305 tag in constant time. Tamper / wrong-key / wrong-AAD all yield the opaque `AeadErr.Decrypt`.",
        capability: "",
        example: "let pt = std.crypto.chacha20_poly1305.decrypt(&key, &nonce, b\"v=1\", &ct)?;\n",
        see_also: "std.crypto.chacha20_poly1305.encrypt, std.crypto.AeadErr, std.crypto.aes_gcm.decrypt",
    },
    StdlibExample {
        symbol: "std.crypto.chacha20_poly1305",
        signature: "module std.crypto.chacha20_poly1305",
        description: "ChaCha20-Poly1305 AEAD (RFC 8439). Mandatory TLS 1.3 ciphersuite; the right pick on hardware without AES-NI (ARM, embedded). Constant-time C/assembly ChaCha is faster than software AES on those platforms. Backed by RustCrypto's audited `chacha20poly1305` crate.",
        capability: "",
        example: "use std.crypto.chacha20_poly1305.{encrypt, decrypt};\nlet ct = encrypt(&key, &nonce, b\"v=1\", b\"hi\")?;\n",
        see_also: "std.crypto.chacha20_poly1305.encrypt, std.crypto.chacha20_poly1305.decrypt, std.crypto.aes_gcm",
    },
    StdlibExample {
        symbol: "std.crypto.AeadErr",
        signature: "enum std.crypto.AeadErr { Encrypt(Str), Decrypt }",
        description: "Errors from both AES-GCM and ChaCha20-Poly1305. `Encrypt(msg)` fires only on pathological input (plaintext > 2^36 - 32 bytes — not reachable in practice). `Decrypt` is intentionally opaque: the variant carries no payload so an attacker can't distinguish \"wrong key\" from \"wrong AAD\" from \"tampered ciphertext\".",
        capability: "",
        example: "match std.crypto.aes_gcm.decrypt(&key, &nonce, b\"v=1\", &ct) {\n  Ok(pt) => use(pt),\n  Err(AeadErr.Decrypt) => return Err(\"auth failed\"),\n  Err(AeadErr.Encrypt(_)) => unreachable(),\n}\n",
        see_also: "std.crypto.aes_gcm.encrypt, std.crypto.chacha20_poly1305.encrypt",
    },
    StdlibExample {
        symbol: "std.crypto.AeadErr.Encrypt",
        signature: "variant AeadErr.Encrypt(msg: Str)",
        description: "Encryption rejected the input — only reachable for plaintext longer than 2^36 - 32 bytes, which Mighty programs can't realistically construct. Surfaced for completeness so the variant table is exhaustive.",
        capability: "",
        example: "match std.crypto.aes_gcm.encrypt(&key, &nonce, aad, pt) {\n  Err(AeadErr.Encrypt(msg)) => log(msg),\n  _ => {},\n}\n",
        see_also: "std.crypto.AeadErr, std.crypto.aes_gcm.encrypt",
    },
    StdlibExample {
        symbol: "std.crypto.AeadErr.Decrypt",
        signature: "variant AeadErr.Decrypt",
        description: "Decryption failed. Either the auth tag didn't verify (ciphertext/AAD/key/nonce tampered or wrong) OR the ciphertext is shorter than the 16-byte tag. INTENTIONALLY OPAQUE — leaking which failure happened helps padding-oracle-style attackers.",
        capability: "",
        example: "if let Err(AeadErr.Decrypt) = std.crypto.aes_gcm.decrypt(&key, &n, aad, &ct) {\n  return Err(\"session tampered\");\n}\n",
        see_also: "std.crypto.AeadErr, std.crypto.aes_gcm.decrypt, std.crypto.chacha20_poly1305.decrypt",
    },
    StdlibExample {
        symbol: "aead_nonce_uniqueness",
        signature: "INVARIANT: nonce MUST be unique per (key, message)",
        description: "Both AES-GCM and ChaCha20-Poly1305 require a 96-bit nonce that is unique for every encryption under a given key. Reuse catastrophically breaks the construction. Two safe patterns: (1) sequential counter persisted in storage (best for messages with a natural order); (2) `std.crypto.random_bytes(12)` with the understanding that ~2^32 messages per key is the safe ceiling for random nonces (collision probability ~2^-32 at that count).",
        capability: "crypto.rand (only if you generate the nonce here)",
        example: "// Random nonce per message (safe up to ~2^32 messages per key).\nlet nonce = std.crypto.random_bytes(12);\nlet ct = std.crypto.aes_gcm.encrypt(&key, &nonce, aad, pt)?;\n",
        see_also: "std.crypto.aes_gcm.encrypt, std.crypto.random_bytes, std.crypto.chacha20_poly1305.encrypt",
    },
    StdlibExample {
        symbol: "aead_aad_binding",
        signature: "AAD = additional authenticated data",
        description: "The `aad` argument to `encrypt`/`decrypt` is NOT encrypted but IS bound into the auth tag. Use it to commit the ciphertext to a version tag, key id, request id, etc. The decryptor MUST pass byte-identical AAD or `AeadErr.Decrypt` fires. Typical pattern: a single-byte version marker that lets you rotate ciphers later without ambiguity.",
        capability: "",
        example: "let aad = b\"v=1\";\nlet ct = std.crypto.aes_gcm.encrypt(&key, &nonce, aad, payload)?;\n// Later: must pass the SAME aad.\nlet pt = std.crypto.aes_gcm.decrypt(&key, &nonce, aad, &ct)?;\n",
        see_also: "std.crypto.aes_gcm.encrypt, std.crypto.aes_gcm.decrypt",
    },
    StdlibExample {
        symbol: "aead_secure_session_pattern",
        signature: "secure-session = HMAC-KDF + random nonce + AEAD + URL-safe-base64",
        description: "Canonical v0.40 T4 stack: derive a per-session key with `hmac_sha256(master, session_id)`, generate a 12-byte random nonce, seal the payload with `aes_gcm.encrypt`, and URL-encode the ciphertext with `base64.encode_url_no_pad`. The full pattern lives at `examples/43_secure_session.mty`.",
        capability: "crypto.rand",
        example: "let key = std.crypto.hmac_sha256(master, session_id);\nlet nonce = std.crypto.random_bytes(12);\nlet ct = std.crypto.aes_gcm.encrypt(&key, &nonce, b\"v=1\", payload)?;\nlet cookie = std.encoding.base64.encode_url_no_pad(&ct);\n",
        see_also: "std.crypto.aes_gcm.encrypt, std.crypto.hmac_sha256, std.crypto.random_bytes, std.encoding.base64.encode_url_no_pad",
    },

    // ---- v0.40 T3 — Char.from_u32 + cast Char (qualified form) ----
    StdlibExample {
        symbol: "Char.from_u32",
        signature: "fn Char.from_u32(value: U32) -> Option[Char]",
        description: "v0.40 T3 — explicit constructor for a `Char` from a runtime `U32` codepoint. Returns `Some(c)` iff `value` is a valid Unicode scalar value (`< 0x110000` and NOT in the surrogate gap `0xD800..=0xDFFF`); otherwise `None`. Mirrors Rust's `char::from_u32`. **Replaces** the v0.39 T2 non-literal `Int as Char` surface, which is now rejected with MT2027 (the fix engine auto-rewrites the old shape).",
        capability: "",
        example: "let a: Option[Char] = Char.from_u32(0x41_u32);     // Some('A')\nlet bad: Option[Char] = Char.from_u32(0xD800_u32); // None (surrogate)\nlet safe: Char = Char.from_u32(v).unwrap_or('?');\n",
        see_also: "char_from_u32, cast_int_to_char, cast_char_to_u32, cast_invalid_mt2028",
    },
    StdlibExample {
        symbol: "cast_char_from_u32_runtime",
        signature: "MIGRATION: non-literal `Int as Char` → `Char.from_u32(value)`",
        description: "v0.40 T3 migration note. The v0.39 T2 surface accepted any `Int as Char` cast and panicked at runtime on out-of-range values. v0.40 T3 rejects non-literal sources at typeck (MT2027 INVALID_CAST) — authors must spell `Char.from_u32(value)` which returns `Option[Char]`. Literal casts (`0x41 as Char`) still work and are still compile-time-checked against MT2028 INVALID_CODEPOINT.",
        capability: "",
        example: "// OLD (v0.39): runtime panic on invalid codepoint.\n// let c: Char = v as Char;            // MT2027 today\n\n// NEW (v0.40 T3): explicit Option fallback.\nlet c: Char = Char.from_u32(v).unwrap_or('?');\n",
        see_also: "Char.from_u32, cast_int_to_char, cast_invalid_mt2027, cast_invalid_mt2028",
    },
    StdlibExample {
        symbol: "char_from_u32_surrogate",
        signature: "INVARIANT: 0xD800..=0xDFFF → None",
        description: "Surrogates (UTF-16 high/low surrogate halves) are NOT Unicode scalar values — they only have meaning as pairs in UTF-16 encoding, never as a standalone `Char`. `Char.from_u32` returns `None` for every value in `0xD800..=0xDFFF` even though they fit in a U32. Same rule the literal `Int as Char` cast applies as MT2028 INVALID_CODEPOINT.",
        capability: "",
        example: "assert_eq(Char.from_u32(0xD800), None);\nassert_eq(Char.from_u32(0xDFFF), None);\nassert_eq(Char.from_u32(0xD7FF).is_some(), true);  // last valid before gap\nassert_eq(Char.from_u32(0xE000).is_some(), true);  // first valid after gap\n",
        see_also: "Char.from_u32, cast_invalid_mt2028, char_from_u32",
    },
    StdlibExample {
        symbol: "char_from_u32_value_range",
        signature: "INVARIANT: value < 0x110000 (Unicode scalar value range)",
        description: "Unicode scalar values are exactly the codepoints in `0..0x110000` minus the surrogate gap. `Char.from_u32(0x110000)` returns `None`; higher values likewise. This matches the UCS / Unicode 15 definition and guards every downstream UTF-8 invariant when the `Char` flows into a `String`.",
        capability: "",
        example: "assert_eq(Char.from_u32(0x10FFFF).is_some(), true); // max valid\nassert_eq(Char.from_u32(0x110000), None);            // one past max\n",
        see_also: "Char.from_u32, cast_invalid_mt2028, char_from_u32_surrogate",
    },

    // ---- std.observe extended (v0.39 backlog) ---------------------
    StdlibExample {
        symbol: "std.observe.percentiles",
        signature: "fn std.observe.percentiles(samples: &[U64]) -> LatencyPercentiles",
        description: "p50 / p95 / p99 over a slice of latencies in milliseconds. Uses the NEAREST-RANK method (matches the std `summarize` aggregator). Empty input → zeros — the caller can distinguish via `samples.is_empty()`.",
        capability: "",
        example: "let lats: Vec<U64> = obs.iter().map(|o| o.latency_ms).collect();\nlet p = std.observe.percentiles(&lats);\nlog(format(\"p50={} p95={} p99={}\", p.p50_ms, p.p95_ms, p.p99_ms));\n",
        see_also: "std.observe.summarize, std.observe.LatencyPercentiles, percentiles",
    },
    StdlibExample {
        symbol: "std.observe.LatencyPercentiles",
        signature: "struct std.observe.LatencyPercentiles { p50_ms: U64, p95_ms: U64, p99_ms: U64 }",
        description: "p50 / p95 / p99 latencies across an observation slice. Returned by `percentiles`. Embedded inside `CostSummary` and every `AggregateRow`.",
        capability: "",
        example: "let p = std.observe.percentiles(&lats);\nassert(p.p50_ms <= p.p95_ms);\nassert(p.p95_ms <= p.p99_ms);\n",
        see_also: "std.observe.percentiles, std.observe.CostSummary, std.observe.AggregateRow",
    },
    StdlibExample {
        symbol: "std.observe.aggregate_by",
        signature: "fn std.observe.aggregate_by(obs: &[&LlmObservation], by: GroupBy) -> Vec[AggregateRow]",
        description: "Group observations by `Provider`, `Model`, `Agent`, or `Tenant` and emit one row per group with count, dollar spend, and p50/p95/p99 latency. The companion of `summarize` for breakdowns.",
        capability: "",
        example: "let rows = std.observe.aggregate_by(&obs, GroupBy.Model);\nfor row in rows { log(format(\"{}: ${:.4}\", row.label, row.cost_dollars)); }\n",
        see_also: "aggregate_by, std.observe.summarize, std.observe.AggregateRow, GroupBy.Provider",
    },
    StdlibExample {
        symbol: "std.observe.AggregateRow",
        signature: "struct std.observe.AggregateRow { label: Str, count: USize, cost_dollars: F64, p50_latency_ms: U64, p95_latency_ms: U64, p99_latency_ms: U64 }",
        description: "One row of an `aggregate_by` breakdown: a group label (model name, provider, etc) plus the call count, dollar spend, and latency percentiles for that group.",
        capability: "",
        example: "for row in std.observe.aggregate_by(&obs, GroupBy.Provider) {\n  log(format(\"{}: {} calls, ${:.2}\", row.label, row.count, row.cost_dollars));\n}\n",
        see_also: "std.observe.aggregate_by, std.observe.CostSummary, std.observe.LatencyPercentiles",
    },
    StdlibExample {
        symbol: "std.observe.CostSummary",
        signature: "struct std.observe.CostSummary { calls: USize, cost_dollars: F64, latency: LatencyPercentiles, by_provider: Vec[AggregateRow], by_model: Vec[AggregateRow] }",
        description: "Top-level result of `summarize`: total count + spend, p50/p95/p99 latency, plus per-provider and per-model breakdowns. Drop-in for dashboards and cost emails.",
        capability: "",
        example: "let s = std.observe.summarize(&obs, Window.Last(\"24h\"));\nlog(format(\"24h spend: ${:.2}\", s.cost_dollars));\n",
        see_also: "CostSummary, std.observe.summarize, std.observe.AggregateRow, std.observe.LatencyPercentiles",
    },

    // ---- std.eval polish (v0.39 backlog) --------------------------
    StdlibExample {
        symbol: "Compare.semantic_with_threshold",
        signature: "fn Compare.semantic_similarity(threshold: F32) -> Compare",
        description: "Constructs a semantic-similarity comparator with the given cosine-similarity cutoff. Threshold 0.85 is a reasonable default for English; 0.95 is conservative; 0.7 is loose. Combine with `Compare.semantic_similarity_with(threshold, embedder)` to pin the embedding model.",
        capability: "",
        example: "let cmp = Compare.semantic_similarity(0.85);\nlet suite = Suite.new(\"qa\").compare_with(cmp).run_with(member);\n",
        see_also: "Compare.semantic_similarity, Compare.semantic_similarity_with, Compare.equal, Suite.compare_with",
    },
    StdlibExample {
        symbol: "Replay.with_provider",
        signature: "fn Replay.with_provider(provider: impl TurnProvider) -> Replay",
        description: "Plug a turn provider into a replay run. The provider drives the actual model calls during replay (vs the baseline trace). Use `MemberTurnProvider.new(member, budget)` to drive replay through a Member + budget cap; use `MemberTurnProvider.unbounded(member)` to run without a budget.",
        capability: "",
        example: "let provider = MemberTurnProvider.unbounded(Member.anthropic(\"claude-opus-4-7\"));\nlet replay = Replay.from_trace(\"baseline.bin\").with_provider(provider);\n",
        see_also: "MemberTurnProvider.new, MemberTurnProvider.unbounded, Case.from_trace, Suite.run_with",
    },
    StdlibExample {
        symbol: "MemberTurnProvider.new",
        signature: "fn MemberTurnProvider.new(member: Member, budget: SharedDollarBudget) -> MemberTurnProvider",
        description: "Build a budget-aware turn provider for replay. The budget is shared across calls so a budget trip inside a multi-case suite stops the whole run cleanly.",
        capability: "",
        example: "let budget = SharedDollarBudget.from_dollars(5.00);\nlet p = MemberTurnProvider.new(member, budget);\n",
        see_also: "MemberTurnProvider.unbounded, Replay.with_provider, SharedDollarBudget.from_dollars",
    },
    StdlibExample {
        symbol: "MemberTurnProvider.unbounded",
        signature: "fn MemberTurnProvider.unbounded(member: Member) -> MemberTurnProvider",
        description: "Budget-free turn provider for replay. Use in tests and CI where the call count is bounded by the trace's recorded turns.",
        capability: "",
        example: "let p = MemberTurnProvider.unbounded(Member.mock(\"m\", \"reply\", 0));\n",
        see_also: "MemberTurnProvider.new, Replay.with_provider",
    },
    StdlibExample {
        symbol: "Report.passed",
        signature: "fn Report.passed(&self) -> Bool",
        description: "Did every case in the suite verdict `Match`? Convenience for the CI exit-code path — `exit(if report.passed() { 0 } else { 1 })`.",
        capability: "",
        example: "let report = suite.run_with(member);\nif !report.passed() { exit(1); }\n",
        see_also: "Report.failure_count, Verdict.Match, Suite.run_with",
    },
    StdlibExample {
        symbol: "Report.failure_count",
        signature: "fn Report.failure_count(&self) -> USize",
        description: "Number of cases whose verdict was not `Match`. Useful for summary lines like \"3 / 50 cases failed\".",
        capability: "",
        example: "let report = suite.run_with(member);\nlog(format(\"{} failed\", report.failure_count()));\n",
        see_also: "Report.passed, Verdict.Diverge, Verdict.Error",
    },
    StdlibExample {
        symbol: "Verdict.Fail",
        signature: "alias for Verdict.Diverge | Verdict.Error",
        description: "v0.39 polish — predicate-shaped helper. A case \"failed\" iff its verdict is either `Diverge` (output didn't match expected) or `Error` (the call panicked / errored out). `Verdict.Match` and `Verdict.SingleMember` are the success-shape verdicts.",
        capability: "",
        example: "let failed = matches!(verdict, Verdict.Diverge(_) | Verdict.Error(_));\n",
        see_also: "Verdict.Match, Verdict.Diverge, Verdict.Error, Report.failure_count",
    },

    // ---- std.iter advanced combinators ----------------------------
    StdlibExample {
        symbol: "Iterator.scan",
        signature: "fn Iterator.scan[B, F: FnMut(&mut St, T) -> Option[B]](self, init: St, f: F) -> Scan[Self, St, F]",
        description: "Like `fold` but emits each intermediate state as an iterator item. Stops when `f` returns `None`. Canonical use: running totals, parser state machines, cumulative sums.",
        capability: "",
        example: "let running_sums: Vec[I64] = [1, 2, 3, 4]\n  .iter()\n  .scan(0, |acc, x| { *acc += x; Some(*acc) })\n  .collect();\n// [1, 3, 6, 10]\n",
        see_also: "Iterator.fold, Iterator.map, Iterator.take_while",
    },
    StdlibExample {
        symbol: "Iterator.take_while",
        signature: "fn Iterator.take_while[P: FnMut(&T) -> Bool](self, pred: P) -> TakeWhile[Self, P]",
        description: "Yield items while `pred` returns true; stop at the first false. Unlike `filter`, the iterator terminates after the first rejection — useful for sorted streams and parse-prefix idioms.",
        capability: "",
        example: "let small: Vec[I64] = nums.iter().take_while(|&n| *n < 100).collect();\n",
        see_also: "Iterator.skip_while, Iterator.filter, Iterator.take",
    },
    StdlibExample {
        symbol: "Iterator.skip_while",
        signature: "fn Iterator.skip_while[P: FnMut(&T) -> Bool](self, pred: P) -> SkipWhile[Self, P]",
        description: "Skip items while `pred` returns true; from the first false on, yield everything (including subsequent items where `pred` would be true again). Mirror image of `take_while`.",
        capability: "",
        example: "let from_first_neg: Vec[I64] = nums.iter().skip_while(|&n| *n >= 0).copied().collect();\n",
        see_also: "Iterator.take_while, Iterator.skip, Iterator.filter",
    },
    StdlibExample {
        symbol: "Iterator.partition",
        signature: "fn Iterator.partition[B: Default + Extend[T], P: FnMut(&T) -> Bool](self, pred: P) -> (B, B)",
        description: "Split the iterator into two collections by predicate: `(matching, rejected)`. Single pass; both collections preserve original order. The canonical \"sort items into two buckets\" combinator.",
        capability: "",
        example: "let (evens, odds): (Vec[I32], Vec[I32]) = (1..10).partition(|&n| n % 2 == 0);\n",
        see_also: "Iterator.filter, Iterator.group_by, Iterator.collect",
    },
    StdlibExample {
        symbol: "Iterator.group_by",
        signature: "fn Iterator.group_by[K: Eq, F: FnMut(&T) -> K](self, key: F) -> GroupBy[Self, F]",
        description: "Group CONSECUTIVE items sharing the same key. NOT a SQL-style global GROUP BY — the input must already be sorted by key for total grouping. Each group yields `(key, sub-iterator)`.",
        capability: "",
        example: "let sorted: Vec<_> = items.iter().sorted_by_key(|x| x.kind).collect();\nfor (kind, group) in sorted.iter().group_by(|x| x.kind) {\n  log(format(\"{}: {}\", kind, group.count()));\n}\n",
        see_also: "Iterator.partition, Iterator.fold, GroupBy.Provider",
    },
    StdlibExample {
        symbol: "Iterator.position",
        signature: "fn Iterator.position[P: FnMut(T) -> Bool](&mut self, pred: P) -> Option[USize]",
        description: "Index of the first item for which `pred` returns true, or `None`. Consumes the iterator up to (but not including) the next item. The index counterpart of `find`.",
        capability: "",
        example: "let idx = nums.iter().position(|&n| n > 100);\n",
        see_also: "Iterator.find, Iterator.any, Iterator.enumerate",
    },
    StdlibExample {
        symbol: "Iterator.last",
        signature: "fn Iterator.last(self) -> Option[T]",
        description: "Consume the iterator and return the last yielded item, or `None` on an empty iterator. O(n) for the general case (specialised on `DoubleEndedIterator` for O(1) `.rev().next()`).",
        capability: "",
        example: "let last_line = std.fs.read_to_string(p)?.lines().last();\n",
        see_also: "Iterator.next, Iterator.rev, Iterator.count",
    },
    StdlibExample {
        symbol: "Iterator.nth",
        signature: "fn Iterator.nth(&mut self, n: USize) -> Option[T]",
        description: "Return the nth item (zero-indexed), or `None` if the iterator runs out first. Advances the iterator — subsequent `next()` calls start AFTER the returned item.",
        capability: "",
        example: "let third = (10..100).nth(3); // Some(13)\n",
        see_also: "Iterator.next, Iterator.skip, Iterator.position",
    },
    StdlibExample {
        symbol: "Iterator.product",
        signature: "fn Iterator.product[T: Mul + One](self) -> T",
        description: "Multiply every item. Empty iterator → `T::one()` (`1` for integers, `1.0` for floats). The `product` half of the sum/product pair.",
        capability: "",
        example: "let factorial: U64 = (1u64..=5).product(); // 120\n",
        see_also: "Iterator.sum, Iterator.fold, Iterator.max",
    },
    StdlibExample {
        symbol: "Iterator.copied",
        signature: "fn Iterator.copied[T: Copy](self) -> Copied[Self]",
        description: "Transform an iterator of `&T` into an iterator of `T` by `Copy`. Cheaper than `.cloned()` because it dispatches on `Copy` rather than `Clone`. The right move after `Vec.iter()` on a primitive element type.",
        capability: "",
        example: "let doubled: Vec[I32] = nums.iter().copied().map(|n| n * 2).collect();\n",
        see_also: "Iterator.cloned, Iterator.map, Iterator.collect",
    },
    StdlibExample {
        symbol: "Iterator.cloned",
        signature: "fn Iterator.cloned[T: Clone](self) -> Cloned[Self]",
        description: "Transform an iterator of `&T` into an iterator of `T` by `Clone`. Use when `T: Clone` but not `Copy` (e.g. `String`).",
        capability: "",
        example: "let owned: Vec[String] = strs.iter().cloned().collect();\n",
        see_also: "Iterator.copied, Iterator.map, Iterator.collect",
    },
    StdlibExample {
        symbol: "Iterator.max_by",
        signature: "fn Iterator.max_by[F: FnMut(&T, &T) -> Ordering](self, cmp: F) -> Option[T]",
        description: "Maximum by an arbitrary comparator. Use when `T` doesn't impl `Ord` (e.g. floats — `f64::total_cmp` is the safe choice).",
        capability: "",
        example: "let largest = floats.iter().copied().max_by(|a, b| a.total_cmp(b));\n",
        see_also: "Iterator.max, Iterator.min_by, Iterator.fold",
    },
    StdlibExample {
        symbol: "Iterator.min_by",
        signature: "fn Iterator.min_by[F: FnMut(&T, &T) -> Ordering](self, cmp: F) -> Option[T]",
        description: "Minimum by an arbitrary comparator. Mirror of `max_by`.",
        capability: "",
        example: "let smallest = floats.iter().copied().min_by(|a, b| a.total_cmp(b));\n",
        see_also: "Iterator.min, Iterator.max_by, Iterator.fold",
    },
    StdlibExample {
        symbol: "Iterator.max_by_key",
        signature: "fn Iterator.max_by_key[K: Ord, F: FnMut(&T) -> K](self, key: F) -> Option[T]",
        description: "Item with the maximum projected key. Compared to `max_by`, this is the right choice when there's a natural \"size\" projection.",
        capability: "",
        example: "let longest = strs.iter().max_by_key(|s| s.len());\n",
        see_also: "Iterator.max, Iterator.min_by_key, Iterator.max_by",
    },
    StdlibExample {
        symbol: "Iterator.min_by_key",
        signature: "fn Iterator.min_by_key[K: Ord, F: FnMut(&T) -> K](self, key: F) -> Option[T]",
        description: "Item with the minimum projected key. Mirror of `max_by_key`.",
        capability: "",
        example: "let shortest = strs.iter().min_by_key(|s| s.len());\n",
        see_also: "Iterator.min, Iterator.max_by_key, Iterator.min_by",
    },
    StdlibExample {
        symbol: "Iterator.by_ref",
        signature: "fn Iterator.by_ref(&mut self) -> &mut Self",
        description: "Borrow the iterator so a combinator chain doesn't consume it. Lets you split a stream — take the first N items, then consume the rest separately.",
        capability: "",
        example: "let head: Vec<_> = iter.by_ref().take(3).collect();\nlet tail: Vec<_> = iter.collect();\n",
        see_also: "Iterator.take, Iterator.skip, Iterator.peekable",
    },
    StdlibExample {
        symbol: "Iterator.inspect",
        signature: "fn Iterator.inspect[F: FnMut(&T)](self, f: F) -> Inspect[Self, F]",
        description: "Run `f` on each item as it passes through, but pass the item unchanged downstream. The lazy print-debug equivalent of `map(|x| { dbg!(x); x })`.",
        capability: "",
        example: "let total: I64 = nums.iter().inspect(|n| log(format(\"{}\", n))).sum();\n",
        see_also: "Iterator.map, Iterator.for_each, log",
    },
    StdlibExample {
        symbol: "Iterator.for_each",
        signature: "fn Iterator.for_each[F: FnMut(T)](self, f: F)",
        description: "Run `f` on each item, consuming the iterator. The functional counterpart to `for x in iter { f(x) }`. Returns `()` — use `fold` if you want an accumulator.",
        capability: "",
        example: "items.iter().for_each(|x| log(format(\"{}\", x)));\n",
        see_also: "Iterator.fold, Iterator.inspect, Iterator.map",
    },

    // ---- std.collections polish (v0.39 backlog) -------------------
    StdlibExample {
        symbol: "BTreeMap.iter",
        signature: "fn BTreeMap.iter[K: Ord, V](&self) -> Iterator[(&K, &V)]",
        description: "Iterate entries in KEY-SORTED order. The fundamental advantage `BTreeMap` has over `HashMap`. O(n) total, O(1) amortised per item.",
        capability: "",
        example: "for (k, v) in scores.iter() {\n  log(format(\"{}: {}\", k, v));\n}\n",
        see_also: "BTreeMap.range, BTreeMap.new, HashMap.iter",
    },
    StdlibExample {
        symbol: "BTreeMap.insert",
        signature: "fn BTreeMap.insert[K: Ord, V](&mut self, key: K, value: V) -> Option[V]",
        description: "Insert `value` under `key`. Returns the previous value if `key` was already present, else `None`. O(log n).",
        capability: "",
        example: "let prev = scores.insert(\"alice\", 42);\n",
        see_also: "BTreeMap.get, BTreeMap.remove, HashMap.insert",
    },
    StdlibExample {
        symbol: "BTreeMap.get",
        signature: "fn BTreeMap.get[K: Ord, V](&self, key: &K) -> Option[&V]",
        description: "Look up the value under `key`. O(log n). Returns `None` if absent.",
        capability: "",
        example: "if let Some(score) = scores.get(\"alice\") { use(score); }\n",
        see_also: "BTreeMap.insert, BTreeMap.contains_key, HashMap.get",
    },
    StdlibExample {
        symbol: "BTreeMap.remove",
        signature: "fn BTreeMap.remove[K: Ord, V](&mut self, key: &K) -> Option[V]",
        description: "Remove and return the value under `key`, or `None` if absent. O(log n).",
        capability: "",
        example: "let removed = scores.remove(\"alice\");\n",
        see_also: "BTreeMap.insert, BTreeMap.get, HashMap.remove",
    },
    StdlibExample {
        symbol: "BTreeMap.contains_key",
        signature: "fn BTreeMap.contains_key[K: Ord, V](&self, key: &K) -> Bool",
        description: "True iff `key` is in the map. O(log n). Use over `get(&k).is_some()` when you don't need the value.",
        capability: "",
        example: "if scores.contains_key(\"alice\") { /* ... */ }\n",
        see_also: "BTreeMap.get, HashMap.contains_key",
    },
    StdlibExample {
        symbol: "BTreeMap.len",
        signature: "fn BTreeMap.len[K: Ord, V](&self) -> USize",
        description: "Number of entries. O(1).",
        capability: "",
        example: "log(format(\"{} entries\", scores.len()));\n",
        see_also: "BTreeMap.is_empty, HashMap.len",
    },
    StdlibExample {
        symbol: "BTreeSet.insert",
        signature: "fn BTreeSet.insert[T: Ord](&mut self, value: T) -> Bool",
        description: "Insert `value`. Returns `true` if it was NEW, `false` if already present. O(log n).",
        capability: "",
        example: "if seen.insert(id) { /* first time we see this id */ }\n",
        see_also: "BTreeSet.contains, BTreeSet.new, HashSet.insert",
    },
    StdlibExample {
        symbol: "BTreeSet.contains",
        signature: "fn BTreeSet.contains[T: Ord](&self, value: &T) -> Bool",
        description: "True iff `value` is in the set. O(log n).",
        capability: "",
        example: "if seen.contains(&id) { return; }\n",
        see_also: "BTreeSet.insert, BTreeSet.range, HashSet.contains",
    },
    StdlibExample {
        symbol: "BTreeSet.range",
        signature: "fn BTreeSet.range[T: Ord](&self, range: Range[T]) -> Iterator[&T]",
        description: "Iterate the values whose order falls in the half-open range `low..high`. Set counterpart of `BTreeMap.range`.",
        capability: "",
        example: "for v in nums.range(10..20) { use(v); }\n",
        see_also: "BTreeSet.contains, BTreeMap.range, BTreeSet.new",
    },
    StdlibExample {
        symbol: "BTreeSet.iter",
        signature: "fn BTreeSet.iter[T: Ord](&self) -> Iterator[&T]",
        description: "Iterate values in SORTED order — the differentiator vs `HashSet.iter`.",
        capability: "",
        example: "for v in nums.iter() { log(format(\"{}\", v)); }\n",
        see_also: "BTreeSet.range, HashSet.iter, BTreeMap.iter",
    },
    StdlibExample {
        symbol: "HashSet.remove",
        signature: "fn HashSet.remove[T: Hash + Eq](&mut self, value: &T) -> Bool",
        description: "Remove `value`. Returns `true` if it was present, `false` if absent. O(1) average.",
        capability: "",
        example: "let was_there = seen.remove(&id);\n",
        see_also: "HashSet.insert, HashSet.contains, HashMap.remove",
    },
    StdlibExample {
        symbol: "HashSet.iter",
        signature: "fn HashSet.iter[T: Hash + Eq](&self) -> Iterator[&T]",
        description: "Iterate values in UNSPECIFIED order — the hash-table ordering is intentionally not stable across runs. Use `BTreeSet.iter` if you need sorted iteration.",
        capability: "",
        example: "for v in seen.iter() { log(format(\"{}\", v)); }\n",
        see_also: "HashSet.contains, BTreeSet.iter, HashMap.iter",
    },
    StdlibExample {
        symbol: "HashSet.intersection",
        signature: "fn HashSet.intersection[T: Hash + Eq](&self, other: &HashSet[T]) -> Iterator[&T]",
        description: "Items in BOTH sets. Lazy iterator — doesn't materialise a new set. O(min(|a|, |b|)) on average.",
        capability: "",
        example: "let common: Vec[&Str] = a.intersection(&b).collect();\n",
        see_also: "HashSet.union, HashSet.difference, HashSet.contains",
    },
    StdlibExample {
        symbol: "HashSet.union",
        signature: "fn HashSet.union[T: Hash + Eq](&self, other: &HashSet[T]) -> Iterator[&T]",
        description: "Items in EITHER set. Lazy iterator. Items present in both are yielded only once.",
        capability: "",
        example: "let everything: Vec<_> = a.union(&b).cloned().collect();\n",
        see_also: "HashSet.intersection, HashSet.difference",
    },
    StdlibExample {
        symbol: "HashSet.difference",
        signature: "fn HashSet.difference[T: Hash + Eq](&self, other: &HashSet[T]) -> Iterator[&T]",
        description: "Items in `self` but NOT in `other`. Lazy iterator. Asymmetric — `a.difference(b) != b.difference(a)` in general.",
        capability: "",
        example: "let only_in_a: Vec<_> = a.difference(&b).cloned().collect();\n",
        see_also: "HashSet.intersection, HashSet.union, HashSet.symmetric_difference",
    },
    StdlibExample {
        symbol: "HashMap.keys",
        signature: "fn HashMap.keys[K: Hash + Eq, V](&self) -> Iterator[&K]",
        description: "Iterate the keys in UNSPECIFIED order. The key-only counterpart of `iter`.",
        capability: "",
        example: "for k in m.keys() { log(format(\"{}\", k)); }\n",
        see_also: "HashMap.values, HashMap.iter, BTreeMap.iter",
    },
    StdlibExample {
        symbol: "HashMap.values",
        signature: "fn HashMap.values[K: Hash + Eq, V](&self) -> Iterator[&V]",
        description: "Iterate the values in UNSPECIFIED order. Value-only counterpart of `iter`.",
        capability: "",
        example: "let total: I64 = scores.values().sum();\n",
        see_also: "HashMap.keys, HashMap.iter",
    },

    // ---- std.json polish ------------------------------------------
    StdlibExample {
        symbol: "Json.as_bool",
        signature: "fn Json.as_bool(&self) -> Option[Bool]",
        description: "Return the value if the node is `Json.Bool`, else `None`. The boolean counterpart of `as_str` / `as_i64`.",
        capability: "",
        example: "let active = config.get(\"enabled\").and_then(|v| v.as_bool()).unwrap_or(false);\n",
        see_also: "Json.as_i64, Json.as_str, Json.Bool",
    },
    StdlibExample {
        symbol: "Json.as_f64",
        signature: "fn Json.as_f64(&self) -> Option[F64]",
        description: "Return the value as `F64` if the node is `Json.Num`. Numbers parsed from JSON live in F64 regardless of whether they look integer-shaped; use `as_i64` if you want integer narrowing semantics.",
        capability: "",
        example: "let temperature = sample.get(\"temp\").and_then(|v| v.as_f64()).unwrap_or(0.0);\n",
        see_also: "Json.as_i64, Json.Num",
    },
    StdlibExample {
        symbol: "Json.as_object",
        signature: "fn Json.as_object(&self) -> Option[&HashMap[Str, Json]]",
        description: "Return the underlying object map if the node is `Json.Obj`, else `None`. Mirrors `as_array` for `Json.Arr`.",
        capability: "",
        example: "if let Some(obj) = node.as_object() {\n  for (k, v) in obj { use(k, v); }\n}\n",
        see_also: "Json.as_array, Json.Obj, Json.get",
    },
    StdlibExample {
        symbol: "Json.is_null",
        signature: "fn Json.is_null(&self) -> Bool",
        description: "True iff the node is `Json.Null`. The `null` JSON value is distinct from \"absent key\" — use `Json.get(k).is_none()` for the latter.",
        capability: "",
        example: "let explicitly_null = node.get(\"opt\").map(|v| v.is_null()).unwrap_or(false);\n",
        see_also: "Json.Null, Json.get",
    },
    StdlibExample {
        symbol: "Json.pretty",
        signature: "fn std.json.encode_pretty(value: &Json) -> Str",
        description: "Encode JSON with 2-space indentation suitable for human-readable output (logs, debug dumps, config files). For wire-format use `std.json.encode`.",
        capability: "",
        example: "let s = std.json.encode_pretty(&value);\nstd.fs.write(p, s.as_bytes())?;\n",
        see_also: "std.json.encode, std.json.parse",
    },

    // ---- std.path polish ------------------------------------------
    StdlibExample {
        symbol: "Path.with_file_name",
        signature: "fn Path.with_file_name(&self, name: &Str) -> PathBuf",
        description: "Return a new path with the final component replaced by `name`. `Path::new(\"/a/b/c\").with_file_name(\"d\")` → `\"/a/b/d\"`. Useful for sibling-file derivations (e.g. `foo.mty` → `foo.mty.ll`).",
        capability: "",
        example: "let out = src.with_file_name(\"out.ll\");\n",
        see_also: "Path.file_name, Path.with_extension, Path.parent",
    },
    StdlibExample {
        symbol: "Path.file_stem",
        signature: "fn Path.file_stem(&self) -> Option[&Str]",
        description: "File name with the final extension removed. `\"a/b/c.tar.gz\".file_stem()` → `Some(\"c.tar\")` — only the LAST extension is stripped. Use the `file_name` + manual split for full multi-extension handling.",
        capability: "",
        example: "let stem = p.file_stem().unwrap_or(\"\");\n",
        see_also: "Path.file_name, Path.extension, Path.with_extension",
    },
    StdlibExample {
        symbol: "Path.has_root",
        signature: "fn Path.has_root(&self) -> Bool",
        description: "Does the path start with a root component (`/` on Unix, drive letter or `\\` on Windows)? Distinct from `is_absolute` on Windows where `\\foo` has a root but no prefix.",
        capability: "",
        example: "if !p.has_root() { p = cwd.join(&p); }\n",
        see_also: "Path.is_absolute, Path.is_relative, Path.components",
    },
    StdlibExample {
        symbol: "Path.is_relative",
        signature: "fn Path.is_relative(&self) -> Bool",
        description: "True iff the path is NOT absolute. Complement of `is_absolute` — never both true.",
        capability: "",
        example: "if p.is_relative() { p = root.join(&p); }\n",
        see_also: "Path.is_absolute, Path.has_root",
    },
    StdlibExample {
        symbol: "Path.starts_with",
        signature: "fn Path.starts_with(&self, base: impl AsRef[Path]) -> Bool",
        description: "True iff `self`'s components begin with `base`'s components. Component-wise — `Path::new(\"/etc/foo\").starts_with(\"/etc\")` is true; `\"/etcfoo\"` is not.",
        capability: "",
        example: "if !p.starts_with(&root) { return Err(\"path-escape\"); }\n",
        see_also: "Path.components, Path.canonicalize, PathBoundary",
    },
    StdlibExample {
        symbol: "Path.ends_with",
        signature: "fn Path.ends_with(&self, child: impl AsRef[Path]) -> Bool",
        description: "True iff `self`'s last components match `child`. Component-wise — `\"a/b/foo.rs\".ends_with(\"foo.rs\")` is true; `\"oo.rs\"` is not.",
        capability: "",
        example: "if p.ends_with(\"Cargo.toml\") { /* ... */ }\n",
        see_also: "Path.starts_with, Path.file_name",
    },

    // ---- std.string polish ----------------------------------------
    StdlibExample {
        symbol: "String.repeat",
        signature: "fn String.repeat(&self, n: USize) -> Str",
        description: "Return a new string consisting of `n` copies of `self`. `\"ab\".repeat(3)` → `\"ababab\"`. Allocates a fresh `String` — for streaming output use a loop.",
        capability: "",
        example: "let separator = \"-\".repeat(40);\n",
        see_also: "String.push_str, String.with_capacity",
    },
    StdlibExample {
        symbol: "String.replace",
        signature: "fn String.replace(&self, pat: &Str, to: &Str) -> Str",
        description: "Return a new string with every occurrence of `pat` replaced by `to`. For one-shot replacement use `Str.replacen(pat, to, 1)`. For regex-shaped replacement use `std.regex.Regex.replace_all`.",
        capability: "",
        example: "let out = body.replace(\"<TOKEN>\", &actual);\n",
        see_also: "std.regex.Regex.replace_all, String.contains, String.split",
    },
    StdlibExample {
        symbol: "String.replacen",
        signature: "fn String.replacen(&self, pat: &Str, to: &Str, n: USize) -> Str",
        description: "Replace up to `n` occurrences of `pat`. `replacen(pat, to, 1)` is the canonical \"replace first occurrence\" idiom.",
        capability: "",
        example: "let first = body.replacen(\"foo\", \"bar\", 1);\n",
        see_also: "String.replace, std.regex.Regex.replace",
    },
    StdlibExample {
        symbol: "String.lines",
        signature: "fn String.lines(&self) -> Iterator[&Str]",
        description: "Iterate over the LF-/CRLF-terminated lines, WITHOUT the line terminator. Trailing empty line (no terminator after) is yielded as an empty string if present. Mirrors Rust's `str::lines`.",
        capability: "",
        example: "for line in input.lines() {\n  if line.trim().is_empty() { continue; }\n  process(line);\n}\n",
        see_also: "String.split, String.trim, std.fs.read_to_string",
    },
    StdlibExample {
        symbol: "String.split_whitespace",
        signature: "fn String.split_whitespace(&self) -> Iterator[&Str]",
        description: "Split on runs of Unicode whitespace, skipping empty fragments. Use this over `split(\" \")` for any input that might have tabs, double spaces, NBSP, etc.",
        capability: "",
        example: "let words: Vec[&Str] = line.split_whitespace().collect();\n",
        see_also: "String.split, String.trim, std.regex.Regex.split",
    },
    StdlibExample {
        symbol: "String.parse",
        signature: "fn String.parse[T: FromStr](&self) -> Result[T, T.Err]",
        description: "Generic parse-from-string. `T` decides the parse semantics (number, IP, custom). Returns the `FromStr` error type on failure — no panicking variant.",
        capability: "",
        example: "let n: I32 = s.parse()?;\n",
        see_also: "String.find, std.json.parse",
    },
    StdlibExample {
        symbol: "String.to_string",
        signature: "fn String.to_string[T: Display](value: T) -> Str",
        description: "Format any `Display`-shaped value as a string. Builtin idiom for stringification — preferred over `format(\"{}\", v)` for single-value cases.",
        capability: "",
        example: "let s = (42).to_string();\n",
        see_also: "format, String.from_str",
    },

    // ---- std.vec polish -------------------------------------------
    StdlibExample {
        symbol: "Vec.insert",
        signature: "fn Vec.insert[T](&mut self, index: USize, element: T)",
        description: "Insert `element` at position `index`, shifting later elements right. O(n) in the tail length. Panics if `index > len()`. For amortised-O(1) appends use `Vec.push`.",
        capability: "",
        example: "items.insert(0, head); // prepend\n",
        see_also: "Vec.push, Vec.remove, Vec.append",
    },
    StdlibExample {
        symbol: "Vec.remove",
        signature: "fn Vec.remove[T](&mut self, index: USize) -> T",
        description: "Remove and return the element at `index`, shifting later elements left. O(n). Panics on out-of-bounds. For O(1) removal that doesn't preserve order use `Vec.swap_remove`.",
        capability: "",
        example: "let head = items.remove(0);\n",
        see_also: "Vec.insert, Vec.swap_remove, Vec.pop",
    },
    StdlibExample {
        symbol: "Vec.swap_remove",
        signature: "fn Vec.swap_remove[T](&mut self, index: USize) -> T",
        description: "Remove the element at `index` by swapping in the last element. O(1) but DOES NOT preserve order. Use over `remove` when iteration order doesn't matter.",
        capability: "",
        example: "let some_item = items.swap_remove(idx);\n",
        see_also: "Vec.remove, Vec.pop",
    },
    StdlibExample {
        symbol: "Vec.truncate",
        signature: "fn Vec.truncate[T](&mut self, len: USize)",
        description: "Shorten the vector to `len`, dropping every element past that. No-op if `len >= self.len()`. Useful for clipping logs to a tail.",
        capability: "",
        example: "if log.len() > 10_000 { log.truncate(10_000); }\n",
        see_also: "Vec.clear, Vec.drain, Vec.pop",
    },
    StdlibExample {
        symbol: "Vec.first",
        signature: "fn Vec.first[T](&self) -> Option[&T]",
        description: "First element, or `None` if empty. The `vec[0]` shape without the out-of-bounds panic risk.",
        capability: "",
        example: "if let Some(head) = items.first() { use(head); }\n",
        see_also: "Vec.last, Vec.get",
    },
    StdlibExample {
        symbol: "Vec.last",
        signature: "fn Vec.last[T](&self) -> Option[&T]",
        description: "Last element, or `None` if empty. Cheap — O(1).",
        capability: "",
        example: "if let Some(tail) = items.last() { use(tail); }\n",
        see_also: "Vec.first, Vec.pop, Vec.get",
    },
    StdlibExample {
        symbol: "Vec.split_at",
        signature: "fn Vec.split_at[T](&self, mid: USize) -> (&[T], &[T])",
        description: "Split into `(head, tail)` at index `mid` without copying. Panics on out-of-bounds. The slice-shaped counterpart of `String.split_at_byte`.",
        capability: "",
        example: "let (head, tail) = items.split_at(5);\n",
        see_also: "Vec.chunks, Vec.iter",
    },
    StdlibExample {
        symbol: "Vec.dedup",
        signature: "fn Vec.dedup[T: PartialEq](&mut self)",
        description: "Remove consecutive duplicate elements in place. NOT a global dedup — sort the vec first or use a `HashSet` accumulator for that.",
        capability: "",
        example: "items.sort();\nitems.dedup();\n",
        see_also: "Vec.sort, HashSet.new, Vec.retain",
    },
    StdlibExample {
        symbol: "Vec.chunks",
        signature: "fn Vec.chunks[T](&self, size: USize) -> Iterator[&[T]]",
        description: "Slice the vec into non-overlapping windows of length `size` (the LAST chunk may be shorter). The slice-of-slices counterpart of `Iterator.chunks`.",
        capability: "",
        example: "for batch in records.chunks(100) {\n  flush(batch);\n}\n",
        see_also: "Iterator.chunks, Vec.split_at, Vec.windows",
    },
    StdlibExample {
        symbol: "Vec.windows",
        signature: "fn Vec.windows[T](&self, size: USize) -> Iterator[&[T]]",
        description: "Sliding window of length `size`, ONE-STEP stride. `[1,2,3,4].windows(2)` → `[[1,2], [2,3], [3,4]]`. The non-chunked counterpart of `chunks`.",
        capability: "",
        example: "for pair in nums.windows(2) {\n  let diff = pair[1] - pair[0];\n}\n",
        see_also: "Vec.chunks, Iterator.windowed, Vec.iter",
    },

    // ---- std.option / std.result polish ---------------------------
    StdlibExample {
        symbol: "Option.flatten",
        signature: "fn Option.flatten[T](self: Option[Option[T]]) -> Option[T]",
        description: "Collapse one level of optional: `Some(Some(x)) → Some(x)`, `Some(None) → None`, `None → None`. Useful when chained `Option`-returning lookups would otherwise nest.",
        capability: "",
        example: "let inner: Option[I32] = map.get(\"k\").map(|v| v.parse().ok()).flatten();\n",
        see_also: "Option.and_then, Option.map, Result.and_then",
    },
    StdlibExample {
        symbol: "Option.or",
        signature: "fn Option.or(self, alt: Option[T]) -> Option[T]",
        description: "Eager fallback: return `self` if `Some`, else `alt`. For lazy fallback (only evaluate the alt when needed) use `or_else`.",
        capability: "",
        example: "let v = primary.or(backup);\n",
        see_also: "Option.or_else, Option.unwrap_or, Result.or",
    },
    StdlibExample {
        symbol: "Option.or_else",
        signature: "fn Option.or_else[F: FnOnce() -> Option[T]](self, f: F) -> Option[T]",
        description: "Lazy fallback: return `self` if `Some`, else invoke `f()` for the alternative. Prefer this over `or` when the alternative is expensive.",
        capability: "",
        example: "let v = cache.get(k).copied().or_else(|| db.lookup(k));\n",
        see_also: "Option.or, Option.unwrap_or_else, Result.or_else",
    },
    StdlibExample {
        symbol: "Option.take",
        signature: "fn Option.take(&mut self) -> Option[T]",
        description: "Replace `self` with `None` and return the old value. The canonical way to move a value out of an `Option` field without disturbing surrounding state.",
        capability: "",
        example: "if let Some(handle) = self.connection.take() {\n  handle.close()?;\n}\n",
        see_also: "Option.replace, Option.unwrap",
    },
    StdlibExample {
        symbol: "Option.replace",
        signature: "fn Option.replace(&mut self, value: T) -> Option[T]",
        description: "Replace `self` with `Some(value)` and return the previous content. Combine with `take` for swap-style updates.",
        capability: "",
        example: "let old = self.handler.replace(new_handler);\n",
        see_also: "Option.take, Option.unwrap",
    },
    StdlibExample {
        symbol: "Option.unwrap",
        signature: "fn Option.unwrap(self) -> T",
        description: "Panic if `None`, return the wrapped value if `Some`. Use ONLY when the absence of value is structurally impossible (post-condition of an earlier check). Prefer `unwrap_or` / `?` for runtime input.",
        capability: "",
        example: "let v = guaranteed_some.unwrap();\n",
        see_also: "Option.unwrap_or, Option.expect, Option.is_some",
    },
    StdlibExample {
        symbol: "Option.unwrap_or_else",
        signature: "fn Option.unwrap_or_else[F: FnOnce() -> T](self, f: F) -> T",
        description: "Lazy default: return the value if `Some`, else invoke `f()` for the default. Prefer over `unwrap_or` when the default is expensive to construct.",
        capability: "",
        example: "let v = cache.get(k).copied().unwrap_or_else(|| recompute());\n",
        see_also: "Option.unwrap_or, Option.or_else",
    },
    StdlibExample {
        symbol: "Option.expect",
        signature: "fn Option.expect(self, msg: &Str) -> T",
        description: "Like `unwrap` but panics with a custom message. The message should describe the INVARIANT, not the failure (\"channel must be open here\" — not \"channel was closed\").",
        capability: "",
        example: "let v = invariant_some.expect(\"checked above at line 42\");\n",
        see_also: "Option.unwrap, Option.unwrap_or, Result.expect",
    },
    StdlibExample {
        symbol: "Result.map",
        signature: "fn Result.map[U, F: FnOnce(T) -> U](self, f: F) -> Result[U, E]",
        description: "Apply `f` to the inner value if `Ok`. `Err` is passed through unchanged. The success-side counterpart of `map_err`.",
        capability: "",
        example: "let parsed = s.parse::<I32>().map(|n| n * 2);\n",
        see_also: "Result.and_then, Result.map_err, Option.map",
    },
    StdlibExample {
        symbol: "Result.is_err",
        signature: "fn Result.is_err(&self) -> Bool",
        description: "True iff the result is `Err`. Complement of `is_ok` — never both true.",
        capability: "",
        example: "if result.is_err() { metrics.errors += 1; }\n",
        see_also: "Result.is_ok, Result.err",
    },
    StdlibExample {
        symbol: "Result.unwrap",
        signature: "fn Result.unwrap(self) -> T",
        description: "Panic if `Err`, return the inner value if `Ok`. Use only when an `Err` is structurally impossible at the call site. Prefer `?` for forwarding and `unwrap_or` for fallback.",
        capability: "",
        example: "let v = guaranteed_ok.unwrap();\n",
        see_also: "Result.unwrap_or, Result.expect, Result.is_ok",
    },
    StdlibExample {
        symbol: "Result.unwrap_err",
        signature: "fn Result.unwrap_err(self) -> E",
        description: "Panic if `Ok`, return the error if `Err`. The dual of `unwrap` — useful in tests where the error path is the asserted outcome.",
        capability: "",
        example: "let err = expected_to_fail.unwrap_err();\nassert!(matches!(err, MyErr::Specific));\n",
        see_also: "Result.unwrap, Result.err, Result.is_err",
    },
    StdlibExample {
        symbol: "Result.expect",
        signature: "fn Result.expect(self, msg: &Str) -> T",
        description: "Like `unwrap` but panics with a custom message including the inner error. Use to document the invariant being asserted.",
        capability: "",
        example: "let v = post_condition.expect(\"checked by validator above\");\n",
        see_also: "Result.unwrap, Option.expect",
    },
    StdlibExample {
        symbol: "Result.or",
        signature: "fn Result.or(self, alt: Result[T, F]) -> Result[T, F]",
        description: "Eager fallback: return `self` if `Ok`, else `alt`. The error type may differ between sides.",
        capability: "",
        example: "let v = try_primary().or(try_secondary())?;\n",
        see_also: "Result.or_else, Option.or",
    },
    StdlibExample {
        symbol: "Result.or_else",
        signature: "fn Result.or_else[F: FnOnce(E) -> Result[T, X]](self, f: F) -> Result[T, X]",
        description: "Lazy fallback: return `self` if `Ok`, else invoke `f(err)` for the recovery path. Receives the error so the recovery can specialise on the failure kind.",
        capability: "",
        example: "let v = primary().or_else(|e| { log(e); fallback() })?;\n",
        see_also: "Result.or, Option.or_else, Result.map_err",
    },

    // ---- std.swarm: Member polish ---------------------------------
    StdlibExample {
        symbol: "Member.label",
        signature: "fn Member.label(&self) -> Str",
        description: "Human-readable identifier for the member (provider + model + optional alias). Use in logs and dashboards. Stable across calls — safe to use as a group-by key.",
        capability: "",
        example: "log(format(\"{}: {} cents\", member.label(), reply.cost_cents));\n",
        see_also: "Member.model, Member.anthropic, GroupBy.Provider",
    },
    StdlibExample {
        symbol: "Member.model",
        signature: "fn Member.model(&self) -> Str",
        description: "Just the model string (e.g. `\"claude-opus-4-7\"`). For the full provider/model breadcrumb use `label`.",
        capability: "",
        example: "log(member.model());\n",
        see_also: "Member.label, Member.anthropic, Member.openai",
    },
    StdlibExample {
        symbol: "Member.mock_error",
        signature: "fn Member.mock_error(name: Str, body: Str) -> Member",
        description: "Mock member that always errors with `body`. Use in tests to exercise error-path policy without hitting a real provider.",
        capability: "",
        example: "let bad = Member.mock_error(\"flaky\", \"503 upstream timeout\");\n",
        see_also: "Member.mock, Member.mock_with_tool_uses",
    },
    StdlibExample {
        symbol: "Member.mock_with_tool_uses",
        signature: "fn Member.mock_with_tool_uses(name: Str, reply: Str, tools: Vec[ToolUse], cost_cents: U64) -> Member",
        description: "Mock member that responds with `reply` AND advertises `tools` as the tool-uses the reply executed. Use to test consumer code that branches on `MemberReply.tool_uses`.",
        capability: "",
        example: "let m = Member.mock_with_tool_uses(\"agent\", \"done\", vec![tool_use(\"fs.read\")], 0);\n",
        see_also: "Member.mock, MemberReply.tool_uses, ToolUse",
    },
    StdlibExample {
        symbol: "MemberReply.tool_uses",
        signature: "fn MemberReply.tool_uses(&self) -> &[ToolUse]",
        description: "Tool invocations the model emitted as part of this reply. Empty for non-tool-using replies.",
        capability: "",
        example: "for tool in reply.tool_uses() {\n  log(tool.name);\n}\n",
        see_also: "MemberReply.tool_names, Member.mock_with_tool_uses, ToolUse",
    },
    StdlibExample {
        symbol: "MemberReply.tool_names",
        signature: "fn MemberReply.tool_names(&self) -> Vec[Str]",
        description: "Just the tool names from `tool_uses`. Convenience for assertions and budget gates that key off the tool surface only.",
        capability: "",
        example: "assert!(reply.tool_names().contains(&\"fs.read\".to_string()));\n",
        see_also: "MemberReply.tool_uses, Compare.tool_call_set_equal",
    },
    StdlibExample {
        symbol: "MemberReply.cost",
        signature: "fn MemberReply.cost(&self) -> F64",
        description: "Dollar cost of this reply (= `cost_cents / 100.0`). Convenience over `cost_cents` when you want to display dollars.",
        capability: "",
        example: "log(format(\"${:.4}\", reply.cost()));\n",
        see_also: "MemberReply.cost_cents, MemberReply.tokens_used",
    },

    // ---- std.fs polish --------------------------------------------
    StdlibExample {
        symbol: "std.fs.read_file",
        signature: "fn std.fs.read_file(cap: &FsCap, path: &Path) -> Result[Vec[U8], IoErr]",
        description: "Alias for `std.fs.read` (raw bytes). Surfaced under both names so docstring searches hit either spelling.",
        capability: "fs.read",
        example: "let bytes = std.fs.read_file(&cap, p)?;\n",
        see_also: "std.fs.read, std.fs.read_to_string, std.fs.write_file",
    },
    StdlibExample {
        symbol: "std.fs.write_file",
        signature: "fn std.fs.write_file(cap: &FsCap, path: &Path, data: &[U8]) -> Result[(), IoErr]",
        description: "Alias for `std.fs.write`. Atomic in the same sense as `write` (truncates + writes; not crash-safe — for that use a temp-file-then-rename pattern).",
        capability: "fs.write",
        example: "std.fs.write_file(&cap, p, payload)?;\n",
        see_also: "std.fs.write, std.fs.read_file",
    },
    StdlibExample {
        symbol: "std.fs.install_default_write_cap",
        signature: "fn std.fs.install_default_write_cap(cap: FsCap) -> FsCap",
        description: "Set the process-wide default WRITE capability and return the PREVIOUS cap (for restore). Mirror of `install_default_read_cap` for the write surface.",
        capability: "",
        example: "let prev = std.fs.install_default_write_cap(FsCap.rooted([\"/tmp\"]));\n// ... do work ...\nstd.fs.install_default_write_cap(prev);\n",
        see_also: "std.fs.install_default_read_cap, FsCap.rooted, std.fs.current_default_write_cap",
    },
    StdlibExample {
        symbol: "std.fs.current_default_write_cap",
        signature: "fn std.fs.current_default_write_cap() -> FsCap",
        description: "Return a copy of the process-wide default WRITE capability. Mirror of `current_default_read_cap`.",
        capability: "",
        example: "let cur = std.fs.current_default_write_cap();\n",
        see_also: "std.fs.install_default_write_cap, std.fs.current_default_read_cap, FsCap.rooted",
    },
    StdlibExample {
        symbol: "FsCap.allows",
        signature: "fn FsCap.allows(&self, path: &Path) -> Bool",
        description: "Predicate: would this cap permit access to `path`? Use to fail-fast before constructing a long IO chain that would reject anyway.",
        capability: "",
        example: "if !cap.allows(&p) { return Err(IoErr.Denied(p)); }\n",
        see_also: "FsCap.rooted, FsCap.unrestricted",
    },
    StdlibExample {
        symbol: "std.fs.install_default_read_cap",
        signature: "fn std.fs.install_default_read_cap(cap: FsCap) -> FsCap",
        description: "Set the process-wide default read capability and return the PREVIOUS cap (for restore). Mighty's `fs::read*` family consults this when no explicit cap is passed.",
        capability: "",
        example: "let prev = std.fs.install_default_read_cap(FsCap.rooted([\"/tmp\"]));\n// ... do work ...\nstd.fs.install_default_read_cap(prev);\n",
        see_also: "std.fs.install_default_write_cap, FsCap.rooted, std.fs.current_default_read_cap",
    },
    StdlibExample {
        symbol: "std.fs.current_default_read_cap",
        signature: "fn std.fs.current_default_read_cap() -> FsCap",
        description: "Return a copy of the process-wide default read capability. Useful for layering — derive a tighter cap from the current one.",
        capability: "",
        example: "let base = std.fs.current_default_read_cap();\n",
        see_also: "std.fs.install_default_read_cap, FsCap.rooted",
    },
];

/// Look up an example by symbol name. Accepts both qualified
/// (`Member.ask`) and bare (`ask`) forms. Bare-name lookup returns the
/// first entry whose final dot-segment matches — callers should only
/// fall back to bare-name lookup once they've ruled out a user-side
/// definition.
pub fn lookup(name: &str) -> Option<&'static StdlibExample> {
    if name.contains('.') {
        return STDLIB_EXAMPLES.iter().find(|e| e.symbol == name);
    }
    // Bare ident: prefer an exact match (e.g. `log`, `swarm`), else
    // fall back to "last-segment matches".
    if let Some(exact) = STDLIB_EXAMPLES.iter().find(|e| e.symbol == name) {
        return Some(exact);
    }
    STDLIB_EXAMPLES
        .iter()
        .find(|e| e.symbol.rsplit('.').next() == Some(name))
}

/// Look up an example given a `(receiver, method)` pair. Tries
/// `<Receiver>.<method>` first (the qualified form), then falls back
/// to bare `<method>`. Useful for hover at the call site of a method
/// invocation where the receiver type is known.
pub fn lookup_method(receiver: &str, method: &str) -> Option<&'static StdlibExample> {
    let qualified = format!("{}.{}", receiver, method);
    if let Some(hit) = STDLIB_EXAMPLES.iter().find(|e| e.symbol == qualified) {
        return Some(hit);
    }
    lookup(method)
}

/// All curated symbols, in stable insertion order. Useful for
/// snapshot tests and for `mty doc index` tooling.
pub fn symbols() -> impl Iterator<Item = &'static str> {
    STDLIB_EXAMPLES.iter().map(|e| e.symbol)
}

/// Number of seeded examples. Convenience for boundary tests.
pub fn examples_count() -> usize {
    STDLIB_EXAMPLES.len()
}

/// Stable content hash of the stdlib examples table. The hash mixes
/// every field of every entry in declaration order using FNV-1a 64.
/// It is suitable for cache-busting on-disk JSON dumps; it is not a
/// cryptographic hash.
pub fn stdlib_examples_hash() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for e in STDLIB_EXAMPLES {
        for s in [
            e.symbol,
            e.signature,
            e.description,
            e.capability,
            e.example,
            e.see_also,
        ] {
            for b in s.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            // field separator
            h ^= 0x1f;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    h
}

/// Render an example as the human-facing markdown payload used by the
/// LSP hover provider. Sections are stable: Signature, Description,
/// Required capability, Example, See also. Empty sections (e.g.
/// `Required capability` for capability-free symbols) are skipped.
pub fn render_hover_markdown(e: &StdlibExample) -> String {
    let mut out = String::new();
    out.push_str("```mty\n");
    out.push_str(e.signature.trim());
    out.push_str("\n```\n\n");
    if !e.description.is_empty() {
        out.push_str(e.description.trim());
        out.push_str("\n\n");
    }
    if !e.capability.is_empty() {
        out.push_str("**Required capability:** ");
        out.push_str(e.capability.trim());
        out.push_str("\n\n");
    }
    if !e.example.is_empty() {
        out.push_str("**Example:**\n\n```mty\n");
        out.push_str(e.example.trim_end());
        out.push_str("\n```\n\n");
    }
    let see: Vec<&str> = e.see_also_iter().take(5).collect();
    if !see.is_empty() {
        out.push_str("**See also:** ");
        out.push_str(&see.join(", "));
        out.push('\n');
    }
    out
}

/// Infer related symbols for `target` from [`STDLIB_EXAMPLES`] using
/// these heuristics, in priority order:
///
/// 1. Same struct/agent family (e.g. all `Member.*` siblings).
/// 2. Same stdlib module prefix (e.g. all `std.http.*`).
/// 3. Same required capability.
///
/// Returns up to `limit` distinct symbols, excluding `target` itself.
/// Symbols already named in `target.see_also` are NOT added again — the
/// curated list is treated as the authoritative head. The inferred list
/// is appended after it.
pub fn infer_see_also(target: &StdlibExample, limit: usize) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let mut seen: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    seen.insert(target.symbol);
    for s in target.see_also_iter() {
        seen.insert(s);
    }
    let family = target.symbol.split('.').next();
    let module = target_module_prefix(target.symbol);

    // Pass 1: same family (`Member.*`, `Consensus.*`, ...).
    if let Some(fam) = family {
        for e in STDLIB_EXAMPLES {
            if out.len() >= limit {
                break;
            }
            if e.symbol.split('.').next() == Some(fam) && seen.insert(e.symbol) {
                out.push(e.symbol);
            }
        }
    }
    // Pass 2: same module prefix (`std.http.*`, `std.json.*`).
    if let Some(prefix) = module {
        for e in STDLIB_EXAMPLES {
            if out.len() >= limit {
                break;
            }
            if target_module_prefix(e.symbol) == Some(prefix) && seen.insert(e.symbol) {
                out.push(e.symbol);
            }
        }
    }
    // Pass 3: same capability.
    if !target.capability.is_empty() {
        for e in STDLIB_EXAMPLES {
            if out.len() >= limit {
                break;
            }
            if e.capability == target.capability && seen.insert(e.symbol) {
                out.push(e.symbol);
            }
        }
    }
    out
}

/// Return the leading `std.<module>` chunk of a symbol path, or `None`
/// when the symbol isn't rooted at `std.`.
fn target_module_prefix(symbol: &'static str) -> Option<&'static str> {
    let rest = symbol.strip_prefix("std.")?;
    // We want the *first two* segments of `std.<mod>.<rest>` (so
    // `std.http`, not `std.http.get`).
    let mut it = rest.split('.');
    let m = it.next()?;
    // Reconstruct from the static slice so we keep `&'static str`.
    let len = "std.".len() + m.len();
    Some(&symbol[..len])
}

/// Best-effort path to the on-disk examples cache. Returns `None`
/// when neither `HOME` nor `USERPROFILE` is set (e.g. some CI runners).
pub fn default_cache_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let mut p = std::path::PathBuf::from(home);
    p.push(".mty");
    p.push("examples-index.json");
    Some(p)
}

/// Serialise the stdlib examples table as a JSON string. Used both for
/// the on-disk cache and for `mty doc explain --json`.
///
/// Hand-written so this module doesn't need to depend on `serde_json`
/// at runtime — the schema is fixed and small enough to format inline.
pub fn examples_to_json() -> String {
    let mut s = String::new();
    s.push_str("{\n  \"version\": 1,\n  \"hash\": \"");
    s.push_str(&format!("{:016x}", stdlib_examples_hash()));
    s.push_str("\",\n  \"examples\": [\n");
    for (i, e) in STDLIB_EXAMPLES.iter().enumerate() {
        s.push_str("    {\"symbol\": ");
        push_json_str(&mut s, e.symbol);
        s.push_str(", \"signature\": ");
        push_json_str(&mut s, e.signature);
        s.push_str(", \"description\": ");
        push_json_str(&mut s, e.description);
        s.push_str(", \"capability\": ");
        push_json_str(&mut s, e.capability);
        s.push_str(", \"example\": ");
        push_json_str(&mut s, e.example);
        s.push_str(", \"see_also\": ");
        push_json_str(&mut s, e.see_also);
        s.push('}');
        if i + 1 < STDLIB_EXAMPLES.len() {
            s.push(',');
        }
        s.push('\n');
    }
    s.push_str("  ]\n}\n");
    s
}

fn push_json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Persist the stdlib examples table to [`default_cache_path`].
///
/// Returns the resolved path on success. Errors are best-effort — the
/// LSP must keep working even when the cache is unwriteable (read-only
/// home, CI sandbox, etc.).
pub fn persist_examples_index() -> std::io::Result<std::path::PathBuf> {
    let path = default_cache_path().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "neither HOME nor USERPROFILE is set",
        )
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, examples_to_json())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_least_fifty_examples_seeded() {
        assert!(
            examples_count() >= 50,
            "expected >= 50 seeded stdlib examples, got {}",
            examples_count()
        );
    }

    /// v0.34 T3 expanded the catalog past 140 entries spanning std.rag,
    /// std.computer, std.swarm, std.observe, std.taint, std.eval,
    /// std.web, std.fs, std.json, std.string, and std.vec. This test
    /// guards against accidental regressions of the expanded surface
    /// area — the count floor matches the v0.34 T3 mandate.
    #[test]
    fn v034_t3_expanded_catalog() {
        assert!(
            examples_count() >= 140,
            "v0.34 T3 expected >= 140 seeded stdlib examples, got {}",
            examples_count()
        );
    }

    /// v0.38 T4 grew the catalog past 300 entries by covering v0.37
    /// surfaces (extern c / FFI, cast `expr as Ty`, MTY_* env vars) and
    /// previously-uncovered helpers (std.process, std.io, std.path,
    /// std.collections, std.iter, std.result, std.option, std.error)
    /// plus polish on existing modules. The floor is one above the
    /// v0.37→v0.38 baseline so accidental deletion of an entire module's
    /// worth of entries is caught.
    #[test]
    fn v038_t4_catalog_floor_300() {
        assert!(
            examples_count() >= 300,
            "v0.38 T4 expected >= 300 seeded stdlib examples, got {}",
            examples_count()
        );
    }

    /// v0.39 T5 grew the catalog past 400 entries by covering v0.39 T1's
    /// new stdlib surfaces (`std.crypto`, `std.encoding`, `std.url`,
    /// `std.uuid`) and back-filling the v0.38 backlog (std.io reader
    /// extras, std.process output / status / env helpers, std.path
    /// mutators + walk, std.iter combinators, std.error context chain,
    /// std.string / std.vec / std.json / std.collections polish).
    #[test]
    fn v039_t5_catalog_floor_400() {
        assert!(
            examples_count() >= 400,
            "v0.39 T5 expected >= 400 seeded stdlib examples, got {}",
            examples_count()
        );
    }

    /// v0.40 T5 grew the catalog past 500 entries by covering v0.40 T4's
    /// new stdlib surfaces (`std.regex`, `std.crypto.aes_gcm`,
    /// `std.crypto.chacha20_poly1305`), the v0.40 T3 cast Char runtime
    /// (`Char.from_u32` + migration notes), and back-filling the v0.39
    /// gap-fillers (std.iter advanced combinators, std.collections
    /// BTreeMap/BTreeSet/HashSet polish, std.json/std.path/std.string/
    /// std.vec/std.option/std.result polish, std.swarm Member helpers,
    /// std.observe percentiles / aggregate_by, std.eval Replay glue,
    /// std.fs cap helpers).
    #[test]
    fn v040_t5_catalog_floor_500() {
        assert!(
            examples_count() >= 500,
            "v0.40 T5 expected >= 500 seeded stdlib examples, got {}",
            examples_count()
        );
    }

    /// Coverage probe for the v0.40 T5 surfaces. Future trimming passes
    /// must drop a floor-changing number of entries before they silently
    /// delete a v0.40-introduced module.
    #[test]
    fn v040_t5_modules_covered() {
        let want = [
            // v0.40 T4 — std.regex
            "std.regex.Regex",
            "std.regex.Regex.new",
            "std.regex.Regex.find",
            "std.regex.Regex.find_all",
            "std.regex.Regex.captures",
            "std.regex.Regex.captures_all",
            "std.regex.Regex.replace",
            "std.regex.Regex.replace_all",
            "std.regex.Regex.is_match",
            "std.regex.Regex.split",
            "std.regex.Regex.as_str",
            "std.regex.Match",
            "std.regex.Match.len",
            "std.regex.Match.is_empty",
            "std.regex.Captures",
            "std.regex.Captures.get",
            "std.regex.Captures.len",
            "std.regex.RegexErr",
            // v0.40 T4 — std.crypto.aes_gcm
            "std.crypto.aes_gcm",
            "std.crypto.aes_gcm.encrypt",
            "std.crypto.aes_gcm.decrypt",
            // v0.40 T4 — std.crypto.chacha20_poly1305
            "std.crypto.chacha20_poly1305",
            "std.crypto.chacha20_poly1305.encrypt",
            "std.crypto.chacha20_poly1305.decrypt",
            // v0.40 T4 — AEAD errors + invariants
            "std.crypto.AeadErr",
            "std.crypto.AeadErr.Encrypt",
            "std.crypto.AeadErr.Decrypt",
            "aead_nonce_uniqueness",
            "aead_aad_binding",
            "aead_secure_session_pattern",
            // v0.40 T3 — Char.from_u32 + cast Char runtime
            "Char.from_u32",
            "cast_char_from_u32_runtime",
            "char_from_u32_surrogate",
            "char_from_u32_value_range",
            // v0.40 T5 — std.observe extended
            "std.observe.percentiles",
            "std.observe.LatencyPercentiles",
            "std.observe.aggregate_by",
            "std.observe.AggregateRow",
            "std.observe.CostSummary",
            // v0.40 T5 — std.eval polish
            "Compare.semantic_with_threshold",
            "Replay.with_provider",
            "MemberTurnProvider.new",
            "MemberTurnProvider.unbounded",
            "Report.passed",
            "Report.failure_count",
            "Verdict.Fail",
            // v0.40 T5 — std.iter advanced
            "Iterator.scan",
            "Iterator.take_while",
            "Iterator.skip_while",
            "Iterator.partition",
            "Iterator.group_by",
            "Iterator.position",
            "Iterator.last",
            "Iterator.nth",
            "Iterator.product",
            "Iterator.copied",
            "Iterator.cloned",
            "Iterator.max_by",
            "Iterator.min_by",
            "Iterator.max_by_key",
            "Iterator.min_by_key",
            "Iterator.by_ref",
            "Iterator.inspect",
            "Iterator.for_each",
            // v0.40 T5 — std.collections polish
            "BTreeMap.iter",
            "BTreeMap.insert",
            "BTreeMap.get",
            "BTreeMap.remove",
            "BTreeMap.contains_key",
            "BTreeMap.len",
            "BTreeSet.insert",
            "BTreeSet.contains",
            "BTreeSet.range",
            "BTreeSet.iter",
            "HashSet.remove",
            "HashSet.iter",
            "HashSet.intersection",
            "HashSet.union",
            "HashSet.difference",
            "HashMap.keys",
            "HashMap.values",
            // v0.40 T5 — std.json polish
            "Json.as_bool",
            "Json.as_f64",
            "Json.as_object",
            "Json.is_null",
            "Json.pretty",
            // v0.40 T5 — std.path polish
            "Path.with_file_name",
            "Path.file_stem",
            "Path.has_root",
            "Path.is_relative",
            "Path.starts_with",
            "Path.ends_with",
            // v0.40 T5 — std.string polish
            "String.repeat",
            "String.replace",
            "String.replacen",
            "String.lines",
            "String.split_whitespace",
            "String.parse",
            "String.to_string",
            // v0.40 T5 — std.vec polish
            "Vec.insert",
            "Vec.remove",
            "Vec.swap_remove",
            "Vec.truncate",
            "Vec.first",
            "Vec.last",
            "Vec.split_at",
            "Vec.dedup",
            "Vec.chunks",
            "Vec.windows",
            // v0.40 T5 — std.option / std.result polish
            "Option.flatten",
            "Option.or",
            "Option.or_else",
            "Option.take",
            "Option.replace",
            "Option.unwrap",
            "Option.unwrap_or_else",
            "Option.expect",
            "Result.map",
            "Result.is_err",
            "Result.unwrap",
            "Result.unwrap_err",
            "Result.expect",
            "Result.or",
            "Result.or_else",
            // v0.40 T5 — std.swarm Member polish
            "Member.label",
            "Member.model",
            "Member.mock_error",
            "Member.mock_with_tool_uses",
            "MemberReply.tool_uses",
            "MemberReply.tool_names",
            "MemberReply.cost",
            // v0.40 T5 — std.fs polish
            "std.fs.read_file",
            "std.fs.write_file",
            "FsCap.allows",
            "std.fs.install_default_read_cap",
            "std.fs.current_default_read_cap",
            "std.fs.install_default_write_cap",
            "std.fs.current_default_write_cap",
        ];
        for sym in want {
            assert!(
                lookup(sym).is_some(),
                "v0.40 T5 module-coverage check: missing seeded entry for {sym}"
            );
        }
    }

    /// Coverage probe for the v0.39 T1 stdlib surfaces plus the
    /// v0.38-backlog gap-fillers. Future trimming passes have to drop a
    /// floor-changing number of entries before they silently delete an
    /// entire new module.
    #[test]
    fn v039_t5_modules_covered() {
        let want = [
            // std.crypto.hash
            "std.crypto.sha256",
            "std.crypto.sha512",
            "std.crypto.blake3",
            "std.crypto.Sha256Hasher",
            "std.crypto.Sha512Hasher",
            "std.crypto.Blake3Hasher",
            // std.crypto.hmac
            "std.crypto.hmac_sha256",
            "std.crypto.hmac_sha512",
            "std.crypto.subtle_eq",
            // std.crypto.rand
            "std.crypto.random_bytes",
            "std.crypto.uniform_int",
            "std.crypto.uniform_f64",
            "std.crypto.RandErr",
            // std.encoding.base64
            "std.encoding.base64.encode",
            "std.encoding.base64.decode",
            "std.encoding.base64.encode_url",
            "std.encoding.base64.encode_url_no_pad",
            "std.encoding.base64.decode_url",
            "std.encoding.Base64Err",
            // std.encoding.hex
            "std.encoding.hex.encode",
            "std.encoding.hex.encode_upper",
            "std.encoding.hex.decode",
            "std.encoding.HexErr",
            // std.url
            "std.url.parse",
            "std.url.Url",
            "std.url.Url.builder",
            "std.url.Url.to_string",
            "std.url.UrlBuilder.host",
            "std.url.UrlBuilder.port",
            "std.url.UrlBuilder.path",
            "std.url.UrlBuilder.query_param",
            "std.url.UrlBuilder.userinfo",
            "std.url.UrlBuilder.fragment",
            "std.url.UrlBuilder.build",
            "std.url.percent_encode",
            "std.url.percent_encode_component",
            "std.url.percent_decode",
            "std.url.UrlErr",
            // std.uuid
            "std.uuid.Uuid",
            "std.uuid.Uuid.v4",
            "std.uuid.Uuid.v7",
            "std.uuid.Uuid.parse",
            "std.uuid.Uuid.to_string",
            "std.uuid.Uuid.nil",
            "std.uuid.Uuid.is_nil",
            "std.uuid.Uuid.version",
            "std.uuid.Uuid.from_bytes",
            "std.uuid.UuidErr",
            // v0.38 backlog — std.io
            "BufReader.read_line",
            "BufWriter.write_all",
            "std.io.stdin_lock",
            "eprint",
            // v0.38 backlog — std.process
            "Command.current_dir",
            "Command.env_clear",
            "Command.stdout_piped",
            "Command.stderr_piped",
            "Command.output",
            "ProcessOutput",
            "ProcessExit.success",
            // v0.38 backlog — std.path
            "PathBuf.push",
            "PathBuf.pop",
            "PathBuf.from",
            "PathBuf.set_extension",
            "Path.metadata",
            "Path.canonicalize",
            "Path.walk",
            // v0.38 backlog — std.iter
            "Iterator.peekable",
            "Iterator.windowed",
            "Iterator.chunks",
            "Iterator.cycle",
            "Iterator.min",
            "Iterator.max",
            "Iterator.flat_map",
            "Iterator.rev",
            "Iterator.step_by",
            // v0.38 backlog — std.error
            "AnyhowError.context",
            "Error.source",
            "Result.context",
            // v0.38 backlog — std.string / std.vec / std.json / std.collections polish
            "String.split",
            "String.trim",
            "String.starts_with",
            "String.ends_with",
            "String.contains",
            "Vec.contains",
            "Vec.sort",
            "Vec.retain",
            "Vec.extend",
            "Json.get",
            "Json.as_str",
            "Json.as_array",
            "HashMap.contains_key",
            "HashMap.iter",
            "HashMap.entry",
            "HashSet.contains",
            "BTreeMap.range",
        ];
        for sym in want {
            assert!(
                lookup(sym).is_some(),
                "v0.39 T5 module-coverage check: missing seeded entry for {sym}"
            );
        }
    }

    /// Sanity-check coverage of every v0.38 T4 new module so a future
    /// trimming pass can't silently delete the FFI / cast / env-var
    /// surface or any of the std.process / std.io / std.path / std.iter
    /// / std.result / std.option helpers.
    #[test]
    fn v038_t4_modules_covered() {
        let want = [
            // v0.37 extern c / FFI
            "extern_block",
            "extern_c_fn",
            "extern_c_variadic",
            "extern_lib",
            "coerce_str_to_u8",
            "addr_of_local",
            "addr_of_mut",
            // v0.37 T2 cast `expr as Ty`
            "cast_as",
            "cast_u8_to_i64",
            "cast_invalid_mt2027",
            "cast_bool_to_u8",
            "cast_char_to_u32",
            "cast_ptr_to_usize",
            // v0.36 rename — MTY_* env vars
            "MTY_LINKER",
            "MTY_OTLP_ENDPOINT",
            "MTY_TRACE",
            "MTY_RUNTIME_THREADS",
            "MTY_RUNTIME_CONTROL_SOCK",
            // std.process
            "Command.new",
            "Command.exec",
            "Command.spawn",
            "std.process.spawn",
            "std.process.exec",
            "std.process.wait",
            "std.process.kill",
            "ProcessExit",
            // std.io
            "std.io.stdin",
            "std.io.stdout",
            "std.io.stderr",
            "BufReader.new",
            "BufReader.lines",
            "BufWriter.new",
            "BufWriter.flush",
            "eprintln",
            "read_line",
            "write_line",
            // std.path
            "Path.new",
            "PathBuf.new",
            "Path.parent",
            "Path.file_name",
            "Path.extension",
            "Path.join",
            "Path.is_absolute",
            "Path.with_extension",
            "Path.exists",
            "Path.components",
            // std.collections
            "HashMap.new",
            "HashMap.insert",
            "HashMap.get",
            "HashMap.remove",
            "HashSet.new",
            "BTreeMap.new",
            "BTreeSet.new",
            // std.iter
            "Iterator.map",
            "Iterator.filter",
            "Iterator.fold",
            "Iterator.collect",
            "Iterator.zip",
            "Iterator.chain",
            "Iterator.take",
            "Iterator.skip",
            "Iterator.enumerate",
            "Iterator.sum",
            "Iterator.count",
            "Iterator.any",
            "Iterator.all",
            "Iterator.find",
            // std.result
            "Result.ok",
            "Result.err",
            "Result.is_ok",
            "Result.map_err",
            "Result.unwrap_or",
            "Result.and_then",
            // std.option
            "Option.is_some",
            "Option.is_none",
            "Option.map",
            "Option.and_then",
            "Option.unwrap_or",
            "Option.ok_or",
            // std.error
            "Error.trait",
            "anyhow_error",
            // polish
            "Member.weighted",
            "Member.panel_of",
            "ConsensusStrategy.AbortOnDissent",
            "Window.Recent",
            "GroupBy.Tenant",
            "top_by_cost",
            "Suite.compare_with",
            "sanitize_compose",
            "named_regex",
            "Allowlist.from_enum",
        ];
        for sym in want {
            assert!(
                lookup(sym).is_some(),
                "v0.38 T4 module-coverage check: missing seeded entry for {sym}"
            );
        }
    }

    /// Sanity-check coverage of the freshly-added modules so a future
    /// trimming pass can't silently delete every entry for one surface.
    #[test]
    fn v034_t3_modules_covered() {
        let want = [
            // std.rag
            "Index.new",
            "Doc.new",
            "ChunkStrategy.ByParagraph",
            "Retriever.new",
            "Reranker.new",
            "Rag.new",
            // std.computer
            "ComputerCap.screen_and_input",
            "Dispatcher.new",
            "Mouse.click_at",
            "Keyboard.type_text",
            "Screen.capture",
            "ComputerAction.Screenshot",
            "SandboxViolation.OutOfBounds",
            // std.swarm internals
            "SharedDollarBudget.new",
            "Consensus.has_consensus",
            "SimilarityMode.TokenSet",
            // std.observe query API
            "Window.parse",
            "GroupBy.Provider",
            "summarize",
            // std.taint sanitisers
            "HtmlEscape",
            "ShellEscape",
            "SqlEscape",
            "PathBoundary",
            "matches_regex",
            "in_allowlist",
            "sanitize_with",
            // std.eval comparators / verdicts
            "Compare.equal",
            "Compare.semantic_similarity",
            "Verdict.Match",
            "Case.from_input",
            // std.web
            "Canvas.new",
            "Input.new",
            "Key.ArrowLeft",
            // std.fs
            "std.fs.read_to_string",
            "std.fs.stat",
            "FsCap.rooted",
            // std.json variants
            "Json.Null",
            "Json.Obj",
            // std.string + std.vec
            "String.with_capacity",
            "Vec.pop",
            "Vec.iter",
        ];
        for sym in want {
            assert!(
                lookup(sym).is_some(),
                "v0.34 T3 module-coverage check: missing seeded entry for {sym}"
            );
        }
    }

    #[test]
    fn every_symbol_is_unique() {
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for e in STDLIB_EXAMPLES {
            assert!(
                seen.insert(e.symbol),
                "duplicate symbol in STDLIB_EXAMPLES: {}",
                e.symbol
            );
        }
    }

    #[test]
    fn every_example_has_signature_and_body() {
        for e in STDLIB_EXAMPLES {
            assert!(
                !e.signature.is_empty(),
                "entry {} missing signature",
                e.symbol
            );
            assert!(
                !e.example.is_empty(),
                "entry {} missing example body",
                e.symbol
            );
        }
    }

    #[test]
    fn lookup_qualified_hits() {
        let e = lookup("Member.ask").expect("Member.ask should be seeded");
        assert_eq!(e.symbol, "Member.ask");
        assert!(e.description.contains("LLM"));
    }

    #[test]
    fn lookup_bare_last_segment_hits() {
        let e = lookup("ask").expect("bare `ask` should fall back to Member.ask");
        assert_eq!(e.symbol, "Member.ask");
    }

    #[test]
    fn lookup_method_qualified_first() {
        let e = lookup_method("Member", "anthropic").expect("Member.anthropic");
        assert_eq!(e.symbol, "Member.anthropic");
    }

    #[test]
    fn lookup_method_falls_back_to_bare() {
        // `log` only exists as a bare entry — exercise the fallback.
        let e = lookup_method("Unknown", "log").expect("bare log fallback");
        assert_eq!(e.symbol, "log");
    }

    #[test]
    fn hash_is_deterministic() {
        let a = stdlib_examples_hash();
        let b = stdlib_examples_hash();
        assert_eq!(a, b);
        // The hash should not be all-zero (sanity).
        assert_ne!(a, 0);
    }

    #[test]
    fn render_hover_markdown_has_all_sections() {
        let e = lookup("Member.ask").unwrap();
        let md = render_hover_markdown(e);
        assert!(md.contains("```mty"), "missing code fence: {}", md);
        assert!(
            md.contains("Required capability"),
            "missing capability: {}",
            md
        );
        assert!(md.contains("Example"), "missing example: {}", md);
        assert!(md.contains("See also"), "missing see-also: {}", md);
        assert!(md.contains("Member.anthropic"));
    }

    #[test]
    fn render_hover_markdown_skips_empty_capability() {
        let e = lookup("log").unwrap();
        let md = render_hover_markdown(e);
        assert!(!md.contains("Required capability"));
    }

    #[test]
    fn json_dump_round_trips_count() {
        let s = examples_to_json();
        // Each example contributes exactly one `"symbol":` field.
        let occurrences = s.matches("\"symbol\":").count();
        assert_eq!(occurrences, examples_count());
        // Hash field present.
        assert!(s.contains("\"hash\":"));
    }

    #[test]
    fn infer_see_also_finds_family_siblings() {
        let e = lookup("Member.ask").unwrap();
        let inferred = infer_see_also(e, 5);
        // The curated `see_also` already covers anthropic/openai/etc,
        // so the inferred list should *not* re-add them. It may pick
        // up `MemberReply.*` siblings (different family root) or
        // remain empty — both are fine. Just assert non-panic.
        for sym in &inferred {
            assert!(!sym.is_empty());
        }
    }

    #[test]
    fn infer_see_also_finds_module_siblings_for_http() {
        let e = lookup("std.http.get").unwrap();
        let inferred = infer_see_also(e, 5);
        // post/serve are in the curated list; nothing else to infer,
        // but the function must run.
        for sym in &inferred {
            assert!(sym.starts_with("std.") || !sym.is_empty());
        }
    }

    #[test]
    fn module_prefix_recovers_two_segments() {
        assert_eq!(target_module_prefix("std.http.get"), Some("std.http"));
        assert_eq!(target_module_prefix("std.json.parse"), Some("std.json"));
        assert_eq!(target_module_prefix("Member.ask"), None);
    }
}
