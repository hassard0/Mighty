# Reference

Authoritative descriptions of the tooling surface. These pages
describe *what the tools accept and emit*; for the language
semantics they enforce, see the
[language specification](../spec/v1.0-rc.md). For compiler
implementation details see [internals/](../internals/README.md).

## CLI

The `mty` binary dispatches to per-command sub-pages:

- [`mty`](cli/mty.md) — top-level binary overview, global flags, exit codes.
- [`mty new`](cli/mty-new.md) — scaffold a new project (templates: web-game, cli, agent).
- [`mty check`](cli/mty-check.md) — lex / parse / lower / typeck / borrowck / effectck / taintck.
- [`mty run`](cli/mty-run.md) — JIT via Cranelift (interpreter fallback on unsupported shapes); `-- <argv>` passthrough.
- [`mty build`](cli/mty-build.md) — native object + linker, or `--target wasm32-{wasi,web}`.
- [`mty serve`](cli/mty-serve.md) — dev server: build + HTTP + WebSocket reload on file change.
- [`mty fmt`](cli/mty-fmt.md) — canonical formatter (idempotent under fuzz).
- [`mty dump`](cli/mty-dump.md) — inspect intermediate artefacts (CST, AST, HIR, SIR).
- [`mty explain`](cli/mty-explain.md) — Cause / Example / Fix / Spec for any `MTxxxx` diagnostic.
- [`mty doc`](cli/mty-doc.md) — markdown / HTML docs from `///` comments.
- [`mty lsp`](cli/mty-lsp.md) — LSP 3.17 server over stdio.
- [`mty pkg`](cli/mty-pkg.md) — package manager (resolve / fetch / publish / verify).
- [`mty inspect`](cli/mty-inspect.md) — runtime introspection; `--cost` reads the `std.observe` SQLite store.
- [`mty replay`](cli/mty-replay.md) — replay traces; `--byte-identical` strict mode; `--diff` divergence reporter.
- [`mty reload`](cli/mty-reload.md) — hot-swap an agent's wasm without losing its state.
- [`mty find`](find.md) — capability-tagged search across the stdlib catalog (v0.33).
- [`mty fix`](cli/mty-fix.md) — bulk-apply fix envelopes from `mty check --format json` (v0.35).
- [`mty agent`](cli/mty-agent.md) — spawn / list / kill / send-message to running agents from the CLI.
- [`mty hooks`](cli/mty.md#hooks) — install / uninstall / status of the project's pre-push git hook (v0.34).
- [`mty doc --check`](cli/mty-doc.md) — Strategy B drift gate: compares per-module docstubs to the curated stdlib catalog (v0.35).

`mty test --eval` runs `*.eval.mty` suites against a provider
panel under byte-identical replay — covered under
[`internals/std-eval.md`](../internals/std-eval.md).

## Manifest + registry

- [Manifest format](manifest.md) — the `mighty.toml` schema (workspace, deps, wit, cluster, profiles).
- [Registry](registry.md) — the GitHub-Releases-backed package registry contract.

## Stdlib

Stable surface area of the standard library:

**Core**

- [`std.fs`](stdlib/fs.md) — file-system reads + writes (capability-gated).
- [`std.http`](stdlib/http.md) — HTTP client (capability-gated, P2-direct).
- [`std.json`](stdlib/json.md) — JSON serialise + parse.
- [`std.test`](stdlib/test.md) — built-in test runner.
- [`std.time`](stdlib/time.md) — wall-clock + monotonic + sleep.
- [`std.tls`](stdlib/tls.md) — TLS sockets (rustls-backed).

**LLM-agent stack** (v0.26–v0.30)

- [`std.llm`](stdlib/llm.md) — typed Anthropic / OpenAI / Gemini / Bedrock providers with streaming, tool use, structured outputs, `TokenBudget` short-circuit.
- [`std.mcp`](stdlib/mcp.md) — MCP server (stdio + http) + client; auto-exposes `@tool`-annotated fns.
- [`std.memory`](stdlib/memory.md) — `VectorStore`, `Episodic`, `Working`; deterministic snapshots fold into replay.
- [`std.swarm`](stdlib/swarm.md) — multi-provider consensus under a shared dollar budget; `Majority` / `Plurality` / `Unanimous` / `WeightedVote` / `FirstAgreed`.

## Macros + attributes

- [`@tool` decorator](macros/tool.md) — generates JSON schema descriptor + invoker + register companions for every provider; `cap:` clause enforced by the runtime.

## Other surfaces

- [Diagnostic codes](diagnostics.md) — the `MT0001`–`MT8010` registry.
- [WASI compatibility matrix](wasi.md) — Preview 1 vs Preview 2 surface, adapter status.
- [Telemetry](telemetry.md) — OpenTelemetry spans, `agent.event()`, OTLP wiring.
- [WIT template](wit/template.wit.md) — starting point for `[wit]` sections in your `mighty.toml`.

## Internals + spec

- [Internals index](../internals/README.md) — every compiler crate's per-page deep dive.
- [Language spec v1.0-RC5](../spec/v1.0-rc.md) — normative reference (frozen for v1.0).
- [Spec amendments](../spec/v0.1-amendments.md) — historical per-decision archive.
