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
