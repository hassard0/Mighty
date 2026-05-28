// v0.33 T3 — bundled examples for the playground.
//
// These are the same source files the agent gallery ships
// (`tools/gallery/examples/*`). We mirror them here as a const-bundle so
// the playground doesn't have to fetch anything at startup — the goal is
// 30-second time-to-first-run. Keeping them inline also lets the
// bundler tree-shake unused ones in a future "small build" mode.
//
// When you add an example to the gallery, mirror it here AND in
// `tools/gallery/index.json`. The gallery README explains the flow.

export type Example = {
  id: string;
  title: string;
  summary: string;
  capabilities: string[];
  source: string;
};

const EX_HELLO = `// Welcome to Mighty. The agent-first language.
//
// Hit Run (or Ctrl/Cmd+Enter) — this program parses, type-checks,
// and runs in the interpreter. Try editing it.
fn main() {
  log("hello, Mighty")
}
`;

const EX_TOOL = `// @tool decorates a plain fn so an LLM agent can call it.
// The compiler synthesises the JSON-schema descriptor + invoke shim;
// the runtime registers it with the MCP server on first call.
@tool("Echo a short greeting back to the caller", cap: fs.read)
fn greet(name: Str) -> Str {
  name
}

fn main() {
  let g = greet("world")
  log(g)
}
`;

const EX_SWARM = `// A swarm of three reviewers vote on a piece of code.
// Each Member is a typed LLM panel cell — providers and models stay
// behind the same interface.
package swarm_review

use std.swarm.Member
use std.swarm.Panel

fn _panel() -> Panel {
  Panel.new("review")
    .with(Member.anthropic("claude-opus-4-7"))
    .with(Member.openai("gpt-5"))
    .with(Member.gemini("gemini-2.5-pro"))
}

fn main() {
  log("03_swarm_review: panel constructed")
}
`;

const EX_EVAL = `// std.eval — typed regression harness over the LLM panel.
package eval_demo

use std.eval.Case
use std.eval.Compare
use std.eval.Member
use std.eval.Suite

fn _build_suite() -> Suite {
  let suite = Suite.new("research-agent")
    .case(Case.from_input("What's the population of France?"))
    .case(Case.from_input("Capital of Australia?"))
    .run_with(Member.mock("baseline", "Paris", 1))
    .run_with(Member.mock("challenger", "Paris", 1))
  suite
}

fn main() {
  log("04_eval_suite: typed suite constructed")
}
`;

const EX_TAINT = `// "The only language where prompt injection is a compile error."
//
// Member.ask(...) returns Tainted[Str]. std.fs.write requires an
// untainted argument. Hit Check — the compiler points at the sink.
//
// To make it compile, replace user_input with one of:
//   user_input.matches_regex("^[a-z ]{1,80}$")
//   user_input.in_allowlist[KnownTopic]()
//   user_input.sanitize_with(PathBoundary("./safe/"))
package taint_basics

use std.fs
use std.swarm.Member

fn main() {
  let m = Member.anthropic("claude-opus-4-7")
  let user_input = m.ask("Summarise the day")
  std.fs.write("log.txt", user_input)
}
`;

const EX_OBSERVE = `// std.observe auto-records every LLM call when MTY_OBSERVE=1.
// mty inspect --cost reads the same SQLite for an ASCII rollup.
package observability_demo

fn _simulate_five_llm_turns() -> Str {
  "ran 5 turns under MTY_OBSERVE=1; check ~/.mty/observations.sqlite"
}

fn main() {
  let _summary = _simulate_five_llm_turns()
  log("06_observability: see mty inspect --cost")
}
`;

const EX_COMPUTER_USE = `// Browser operator agent — drives a real Chrome via std.computer.
package computer_use_demo

use std.computer.Browser
use std.swarm.Member

fn _do_research(b: Browser, query: Str) -> Str {
  let _ = b.goto("https://duckduckgo.com")
  let _ = b.type_text("input[name=q]", query)
  let _ = b.click("button[type=submit]")
  b.read_text("main")
}

fn main() {
  log("07_computer_use: browser operator surface")
}
`;

export const EXAMPLES: Example[] = [
  {
    id: "01_hello_agent",
    title: "01 — Hello, Mighty",
    summary: "A one-line program. Parses, type-checks, runs.",
    capabilities: ["parse", "run"],
    source: EX_HELLO,
  },
  {
    id: "02_tool_calling",
    title: "02 — @tool decorator",
    summary: "Tag a fn as an MCP tool; the compiler generates the descriptor.",
    capabilities: ["macros", "mcp"],
    source: EX_TOOL,
  },
  {
    id: "03_swarm_review",
    title: "03 — Multi-LLM swarm review",
    summary: "Build a Panel of three providers behind one interface.",
    capabilities: ["swarm", "llm"],
    source: EX_SWARM,
  },
  {
    id: "04_eval_suite",
    title: "04 — std.eval regression suite",
    summary: "Typed eval cases + comparators across providers.",
    capabilities: ["eval", "llm"],
    source: EX_EVAL,
  },
  {
    id: "05_taint_safety",
    title: "05 — Tainted[T] compile error",
    summary: "Prompt injection caught at compile time. Hit Check.",
    capabilities: ["taint", "compile-error"],
    source: EX_TAINT,
  },
  {
    id: "06_observability",
    title: "06 — std.observe + mty inspect --cost",
    summary: "Auto-record LLM cost/latency to local SQLite.",
    capabilities: ["observability"],
    source: EX_OBSERVE,
  },
  {
    id: "07_computer_use",
    title: "07 — Computer-use browser operator",
    summary: "Drive a real browser from a typed agent.",
    capabilities: ["computer-use", "browser"],
    source: EX_COMPUTER_USE,
  },
];

export function findExample(id: string): Example | undefined {
  return EXAMPLES.find((e) => e.id === id);
}

export const DEFAULT_EXAMPLE_ID = "01_hello_agent";
