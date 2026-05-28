# Frequently Asked Questions

A grab-bag of questions that come up often. If yours isn't here,
open an issue on
[github.com/hassard0/Mighty](https://github.com/hassard0/Mighty).

## Language and design

### Why another systems language?

Mighty bets that "agent-era" software needs a language where
concurrency, authority, failure, memory, and observability are
**compiler-visible semantics** rather than framework conventions.
The intent is *faster than idiomatic C++* on targeted workloads
by making optimisation facts (no aliasing, bounded lifetimes,
known effects, known capabilities) explicit, while staying
*easier than Go* by inferring most local types and offering
compact canonical forms for boilerplate.

See [spec §0 Executive Definition](spec/v1.0-rc.md).

### Why is the compiler in Rust?

Phase 1 prioritised implementation velocity and memory safety.
Per spec §31.2: *"Rust is preferred for implementation velocity
and memory safety."* The compiler is not user-visible — the
language self-hosts the lexer, parser, HIR, and minimal typeck;
once it self-hosts the rest, the bootstrap host stops mattering.

### Why `.mty`?

Short, unambiguous, and globally unused. The v0.7 rebrand renamed
the historical `.sd` extension (originally chosen for *Stardust*)
to `.mty` to match the new project name. Both the source extension
and the diagnostic prefix changed; see the next two entries.

### What happened to `.sd` and `SD####` codes?

The v0.7 rebrand renamed:

| Was              | Is                |
|------------------|-------------------|
| `stardust` repo  | `mighty` repo     |
| `sdust-*` crates | `mty-*` crates    |
| `star.toml`      | `mighty.toml`     |
| `.sd` source ext | `.mty` source ext |
| `SD####` codes   | `MT####` codes    |

The `SD` diagnostic prefix is preserved as an alias inside
`mty explain` per amendment **A107** — your bookmarks to
`mty explain SD0001` keep working. The `.sd` extension was *not*
preserved as an alias; old files have to be renamed.

### Why `mty` and not `mighty`?

A short binary name pays for itself every time you type it. `mty`
is unambiguous and consistent with the other ecosystem
identifiers (`mighty.toml`, `.mty`, `mty pkg`).

### Why are generics in square brackets?

To keep parsing unambiguous with comparison operators and to
remove the need for the turbofish. `Vec[Str]` is unambiguous;
`Vec<Str>` requires lookahead in some contexts. See tour
[chapter 3](tour/03-generics.md) and amendment **A2** for the
expression-position `::[T]` turbofish.

### What's the difference between `1k`, `1K`, `1KiB`, `1KB`?

Per amendment **A1**:

- **Binary suffixes** (`1KiB` = 1024, `1MiB` = 1048576,
  `1GiB` = 2^30) — the only suffixes accepted in memory and
  storage contexts.
- **Decimal suffixes** (`1k` = 1000, `1M` = 1000000) — accepted
  in count contexts (e.g. `mb 1k` for mailbox depth).
- `KB` / `MB` / `GB` (uppercase decimal) and lowercase `kib` etc.
  are rejected by the lexer. Use the canonical forms above.

`mty explain MT0004` covers duration units in the same vein.

### What's the difference between an agent and an Erlang process?

Conceptually very similar — isolated state, mailbox-driven,
supervised by a tree. The main differences are static typing
(protocols), capability-based authority (no ambient I/O), and
compile-time visible effects.

### What's the difference between an agent and an actor?

Mighty agents have all four: isolation, asynchrony, typed
protocols, and capability boundaries. Most actor libraries pick
two or three. Spec §2.3 lists the full set: *"isolated state
owner, concurrency unit, failure boundary, capability boundary,
observability boundary, scheduling boundary."*

### What's the difference between an agent and a task?

A *task* is a one-shot future scheduled by the runtime — it runs
to completion and yields a value. An *agent* is long-lived, owns
state, has an inbox, and answers messages over its lifetime. Use
tasks for fire-and-forget background work; use agents for
anything with state or identity.

### Can I use Mighty for embedded or kernel work?

That is the `core` profile. It forbids global GC, dynamic dispatch
by default, and a managed heap. The profile constraints are
enforced; see tour [chapter 10](tour/10-capabilities.md) for the
capability story.

## The v0.27–v0.30 differentiator surface

### How does Mighty prevent prompt injection at compile time?

Through **taint types** (`Tainted[T]`), shipped in v0.30 Track A.
Stdlib LLM / MCP / HTTP / env sources mint
`Tainted[T]` values; sensitive sinks (`fs.write`, `process.exec`,
`sql.execute`, `net.request.body`) require a plain `T`. The
compiler reports **MT4099** at the first call where tainted data
would reach a sink without an approved exit. Three exits are
recognised:

1. A sanitiser fn that consumes `Tainted[T]` and returns `T`.
2. `Untaint::after(value, justification)` with an explicit audit
   string.
3. `@taint_transparent` on a fn declared to be exhaustively
   analysed.

See [`docs/internals/taint-types.md`](internals/taint-types.md)
and example [`examples/33_taint_basics.mty`](https://github.com/hassard0/Mighty/blob/main/examples/33_taint_basics.mty).

### What does "capability-typed tools" actually mean?

When you write `@tool("read a file", cap: fs.read("./data/**")) fn
read_doc(path: Str) -> Str`, the macro expands into descriptor +
invoker + register companion fns. The `cap` clause is **enforced
by the runtime**, not the prompt: any `std.fs.read` call inside
the tool body that tries to read outside `./data/**` returns
`Result::Err(forbidden: ...)`. A misbehaving LLM cannot escape
the tool's capability set.

See [`docs/reference/macros/tool.md`](reference/macros/tool.md)
and example
[`examples/27_tool_attr.mty`](https://github.com/hassard0/Mighty/blob/main/examples/27_tool_attr.mty).

### What does "deterministic replay" actually mean?

Every Mighty agent run records a per-turn trace (LLM calls,
tool calls, mailbox sends, time / random reads, host-call
returns). `mty replay <trace> --byte-identical` re-executes the
agent against the recorded trace and asserts the new outputs
match byte-for-byte. Any divergence (model nondeterminism,
clock drift, tool-impl bug) is reported with the first divergent
turn pointed at — you don't guess.

The replay machinery underpins `mty test --eval` (v0.30 Track E),
so LLM-agent regression tests are deterministic in the same way
unit tests on pure code are deterministic.

See [`docs/internals/replay.md`](internals/replay.md).

### What does "swarm consensus" actually mean?

`std.swarm` (v0.27 Track D, v0.29 Track A interpreter arm) votes
across LLM providers under a shared `DollarBudget`:

```mty
let panel = vec![
  Member.anthropic(client, "claude-opus-4-7"),
  Member.openai(client, "gpt-5"),
  Member.gemini(client, "gemini-2.0-flash"),
];
let consensus = swarm(prompt, panel, budget, ConsensusStrategy.Majority);
log!("verdict={}, cost={c}c", consensus.majority, consensus.total_cost_cents);
```

Strategies: `Majority`, `Plurality`, `Unanimous`, `WeightedVote`,
`FirstAgreed`. Returns a `Consensus` with `majority`, `dissents`,
`budget_exhausted`. See
[`docs/reference/stdlib/swarm.md`](reference/stdlib/swarm.md) and
demo
[`08_swarm_review`](https://github.com/hassard0/Mighty/blob/main/demos/08_swarm_review/README.md).

### What does `mty inspect --cost` show me?

Under `MTY_OBSERVE=1`, every `Member.ask` / `LlmProvider.complete`
auto-records to a local SQLite store at
`~/.mty/observations.sqlite`. `mty inspect --cost` reads it back:
total $$, per-provider breakdown, p50/p95/p99 latency. Filter
windows with `--since 7d`, regroup with `--by model`, override
pricing with `MTY_PRICING_OVERRIDE=./pricing.toml`.

The per-instance cost column on the SWE-bench page
([`dev/history/benchmarks/swe-bench-smoke-v0.30.md`](https://github.com/hassard0/Mighty/blob/main/dev/history/benchmarks/swe-bench-smoke-v0.30.md))
is generated from this store, not hand-counted.

See [`docs/internals/observability.md`](internals/observability.md).

### What does `@computer_use` do?

Wraps Anthropic Computer Use as a Mighty tool with a
**capability-typed sandbox**. The sandbox bounds are static (must
declare `display: 1280x720`, `allowed_apps: ["chromium"]`, etc.);
the runtime enforces them. A misbehaving Computer Use loop cannot
spawn an arbitrary process or render to an undeclared display.

See
[`examples/36_computer_use.mty`](https://github.com/hassard0/Mighty/blob/main/examples/36_computer_use.mty)
and [`docs/internals/computer-use.md`](internals/computer-use.md).

## Installation

### Why does `mty` fail to link on Windows?

If you see an error mentioning `link.exe` not found or a missing
DLL during `cargo install --path crates/mty-cli`, you're hitting
the MSVC-toolchain dependency. Two fixes:

1. Install the Visual Studio Build Tools (the *Desktop development
   with C++* workload).
2. Or, install the GNU toolchain target:
   `rustup toolchain install stable-x86_64-pc-windows-gnu` and use
   that instead.

If the build succeeds but `mty build` can't find a linker, point
`STARDUST_LINKER` at one or install a C toolchain — see `mty
explain MT8008`.

### What is the macOS `LC_BUILD_VERSION` fix?

macOS 11+ requires every Mach-O object to carry an
`LC_BUILD_VERSION` load command. Cranelift didn't emit one until
v0.10.0 (commit `7f2feab`). If you see a linker warning like
*"object file does not contain platform information"* when running
`mty build`, upgrade to v0.10.0 or later; the warning is benign
on older releases but blocks notarisation.

### What MSRV does Mighty require?

Rust **1.85+**. The CI MSRV gate runs `cargo test --no-run` + a
bedrock test subset to guarantee the floor. The
`rust-toolchain.toml` pins the current build to 1.95.0 (the latest
stable at slice release time), but the MSRV floor stays at 1.85.

### Where are the binary releases?

Pre-built tagged binaries ship on every release at
<https://github.com/hassard0/Mighty/releases> (Linux x86_64,
macOS arm64, Windows x86_64). From-source is still supported via
`cargo install --path crates/mty-cli`.

## Status

### What works today (v0.30)?

- Lexing, parsing, formatting, CST / AST / HIR / SIR dumps.
- Type checker (HM + bidirectional), borrow checker, effect /
  capability / **taint** checker.
- Codegen: Cranelift native (JIT + AOT), WebAssembly (Component
  Model on `wasm32-web`; Preview 2 on `wasm32-wasi`). LLVM behind
  `--features llvm`.
- Runtime: tokio executor + per-agent mailboxes + supervisors +
  budgets + sandboxes + **cluster mesh** + **hot reload** +
  **deterministic replay**.
- Stdlib: `std.{fs, http, json, time, test, tls, llm, mcp,
  memory, swarm, eval, observe, computer, web, fmt}`.
- 36 canonical examples + 9 demos + SWE-bench Verified harness.
- Self-host: lexer, parser, HIR, minimal typeck.
- **3174 tests passing** across the workspace (2502 Rust + 490
  Python 2nd-impl + 159 conformance + 23 self-host driver).

### What's still on the v1.0 path?

The eight tracked RFCs in
[`docs/spec/rfcs/`](https://github.com/hassard0/Mighty/tree/main/docs/spec/rfcs)
with open comment windows. RFC-005 closes earliest (2026-06-09,
14 days); RFC-002 + RFC-006 latest (2026-07-25, 60 days). The
proposed freeze date is 2026-09-01.

There is **no remaining post-v1.0 backlog**. Every former
post-v1.0 item — lossless live agent migration, Polonius borrows,
distributed agents, hot reload, DWARF v5, PGO/ThinLTO,
work-stealing — has landed pre-v1.0.

### Why doesn't `mty check` catch this obviously wrong program?

If a snippet looks wrong but `mty check` says `ok`, check whether
it's a spec-only feature (rare today — most are enforced) or a
known-deferred amendment. The
[conformance corpus](https://github.com/hassard0/Mighty/tree/main/tests/conformance)
is the canonical list of what's enforced.

## Using Mighty

### Is Mighty production ready?

**No.** The spec is RC5 and the toolchain is v0.30 (pre-1.0). It
is stable enough for hobby projects, learning, and demos. It is
not yet stable enough to bet a paycheque on. Open issues for
rough edges; they will be triaged for v1.0.

### Can I use Mighty in a hobby project?

Yes, with two warnings:

1. There's no semver guarantee until v1.0. A `git pull` may break
   your project — pin to a tagged release if that matters.
2. Pre-built binaries exist but the API surface still moves on
   each minor. Read the `CHANGELOG.md` before bumping.

### How do I report a bug?

Open an issue on
[github.com/hassard0/Mighty](https://github.com/hassard0/Mighty)
using one of the templates in `.github/ISSUE_TEMPLATE/`. A
minimal `.mty` reproducer plus the output of `mty --version`
makes triage 10× faster.

### How can I help?

See [contributing.md](contributing.md). The highest-leverage
contributions today:

- **Conformance cases** — every new test case adds normative
  ground truth.
- **Real-world `.mty` programs** — we need a few more
  end-to-end examples in `examples/` and `demos/`.
- **Doc fixes** — if the tour or getting-started misled you, open
  a PR.
- **SWE-bench harness work** — see
  [`bench/swe/README.md`](https://github.com/hassard0/Mighty/blob/main/bench/swe/README.md).
- **RFC comments** — the eight open RFCs are all looking for
  feedback before the 2026-09-01 freeze.

### Where do I find spec amendments?

The amendments register is at
[`docs/spec/v0.1-amendments.md`](spec/v0.1-amendments.md). The
numbered amendments (A1..A109, with gaps) are the historical
record. The consolidated v1.0-RC5 spec at
[`docs/spec/v1.0-rc.md`](spec/v1.0-rc.md) is what RC5 normatively
specifies; the amendments file remains the per-decision archive.
