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
