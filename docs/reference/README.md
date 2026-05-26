# Reference

Authoritative descriptions of the tooling surface. These pages
describe *what the tools accept and emit*; for the language semantics
they enforce, see the [language specification](../spec/v1.0-rc.md).
For compiler implementation details, see
[internals/](../internals/README.md).

## CLI

The `mty` binary dispatches to per-command sub-pages:

- [`mty`](cli/mty.md) — top-level binary overview, global flags, exit codes.
- [`mty new`](cli/mty-new.md) — scaffold a new project.
- [`mty check`](cli/mty-check.md) — lex / parse / lower / typeck / borrowck only.
- [`mty run`](cli/mty-run.md) — JIT (Cranelift) or interpret.
- [`mty build`](cli/mty-build.md) — native object + linker or WASM (core / component).
- [`mty fmt`](cli/mty-fmt.md) — canonical formatter (idempotent under fuzz).
- [`mty dump`](cli/mty-dump.md) — inspect intermediate artifacts (AST, HIR, SIR).
- [`mty explain`](cli/mty-explain.md) — one-paragraph explanation of any MTxxxx diagnostic.
- [`mty doc`](cli/mty-doc.md) — markdown / HTML docs from `///` comments.
- [`mty lsp`](cli/mty-lsp.md) — LSP 3.17 server over stdio.
- [`mty pkg`](cli/mty-pkg.md) — package manager (resolve / fetch / publish / verify).
- [`mty inspect`](cli/mty-inspect.md) — runtime introspection client.
- [`mty replay`](cli/mty-replay.md) — replay traces; byte-identical re-execution.

## Manifest + registry

- [Manifest format](manifest.md) — the `mighty.toml` schema (workspace, deps, wit, cluster, …).
- [Registry](registry.md) — the GitHub-Releases-backed package registry contract.

## Stdlib

Stable surface area of the standard library:

- [`std.fs`](stdlib/fs.md) — file-system reads + writes (capability-gated).
- [`std.http`](stdlib/http.md) — HTTP client (capability-gated, P2-direct).
- [`std.json`](stdlib/json.md) — JSON serialise + parse.
- [`std.test`](stdlib/test.md) — built-in test runner.
- [`std.time`](stdlib/time.md) — wall-clock + monotonic + sleep.
- [`std.tls`](stdlib/tls.md) — TLS sockets (rustls-backed).

## Other surfaces

- [Diagnostic codes](diagnostics.md) — the `MT0001`–`MT8010` registry.
- [WASI compatibility matrix](wasi.md) — Preview 1 vs Preview 2 surface, adapter status.
- [Telemetry](telemetry.md) — OpenTelemetry spans, `agent.event()`, OTLP wiring.
- [WIT template](wit/template.wit.md) — starting point for `[wit]` sections.
