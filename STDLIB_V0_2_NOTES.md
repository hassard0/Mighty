# Stardust stdlib v0.2 — implementation notes

This file documents the choices made while implementing `sdust-stdlib`
in the v0.2 wave-2 slice. The high-level architecture lives in
`docs/internals/stdlib.md`; this is the developer-facing changelog +
known-gaps tracker.

## Architecture choice — Strategy A

We picked **Strategy A** (synthesize bindings + provide real impls in
the host) over **Strategy B** (ship `.sd` source files):

- Faster to ship: no module-resolver work, no `sdust-pkg` integration.
- Lets the real impls (rustls, hyper, serde_json) sit behind a single
  Rust API surface, easy to test with `#[tokio::test]`.
- v0.3 will migrate to Strategy B once `sdust-pkg` lands the
  bundled-package contract.

Concrete wiring:

1. `sdust-types::prelude` already registers `std`, `std.json`,
   `std.tls`, `std.http`, `std.fs`, `std.time`, etc. as opaque module
   names. SIR's `EffectOp::GenericCall { path, method }` lowers any
   `module.method(arg, ...)` call into a typed effect call carrying the
   path segments.
2. `sdust-runtime::host_std::StdHost::effect_call` runs sandbox checks,
   then forwards to a process-wide function pointer registered via
   `sdust_runtime::host_std::install_dispatcher`.
3. `sdust-stdlib::host::install()` registers `sdust_stdlib::host::dispatch`
   as that dispatcher. It pattern-matches on `(module, method)` and
   calls the right Rust impl, returning a `sdust_sir::interp::Value`.

The runtime → stdlib hook is a function pointer (not a trait object)
to dodge the dep-graph cycle: the runtime can't depend on the stdlib
without a cycle through driver, but the stdlib *can* depend on the
runtime.

## Driver wiring (v0.3 task)

For `sdust run` to actually route `std.json.parse` through the real
parser, the driver must call `sdust_stdlib::host::install()` somewhere
in its setup. **We did not do that in v0.2** — the wave-2 work-area
constraints forbid driver edits in this agent. The hook exists; the
v0.3 driver agent flips the switch in a one-line change.

Until then, `sdust run` programs that call `std.*` see a `Value::Unit`
return from the runtime's effect-call sink, matching the slice-7
surface. The stdlib's public Rust API (used directly by other tools
+ tests) carries the real semantics today.

## Crate features

`sdust-stdlib` exposes a `runner` feature (default-on) that pulls
`sdust-driver` + `sdust-diagnostics` for the `std.test` test-runner.
Downstream crates that only want JSON / TLS / HTTP / FS / time can
disable it with `default-features = false`, keeping the dep graph
small and avoiding the driver's transitive `cranelift-codegen`
dependency.

## Module-by-module notes

### `std.json`

- Wraps `serde_json` but exposes a Stardust-shaped `Json` enum with a
  `BTreeMap`-backed object so encoded output is deterministic across
  runs (matters for `std.test` snapshots).
- **Known gap**: `Json::Num` uses `f64`, so integers larger than
  2^53 lose precision on round-trip. Tracked for v0.3 (`Json::Int(i64)`
  + `Json::Uint(u64)` variants).
- `parse`, `encode`, `encode_pretty` are the entire surface.

### `std.tls`

- Built on `rustls` 0.23 + `tokio-rustls` 0.26 with the `ring` crypto
  provider installed via `ensure_crypto_provider()` (idempotent
  `Once`).
- Client: `connect(host, port)` returns a tokio-flavoured
  `TlsStream<TcpStream>` after handshake.
- Server: `acceptor_from_pem(cert, key)` loads PEM-encoded chain +
  PKCS#8 / RSA / SEC1 private key.
- Test fixture: `rcgen` generates a self-signed cert per-test, the
  client trusts it explicitly via `client_config_with_root`.
- **Known gap**: native root cert loading is stubbed
  (`rustls_native_certs_load` returns `Err`). Adding the crate is a
  one-line v0.3 task; we skipped it to keep the compile small.

### `std.http`

- Real HTTP/1.1 client + server via `hyper` 1.x + `hyper-util`.
- Client: `get(url)` / `post(url, body)` return a fully-buffered
  `Response { status, body, headers }`.
- Server: `serve(addr, handler)` binds on `addr`, accepts forever,
  dispatches each request to `handler` via tokio tasks.
- **Known gap — HTTP/2**: the underlying `hyper` version supports
  HTTP/2 (`hyper::server::conn::http2`), but the v0.2 server uses
  `http1::Builder` only. HTTP/2 + ALPN negotiation is a v0.3 task
  (we have the plumbing in `std.tls`).
- **Known gap — `https://`**: the v0.2 client errors with
  `HttpErr::Url` on `https://` URLs. Wiring `hyper-rustls` cleanly
  without dragging a new crate dep is a v0.3 task; the `std.tls`
  primitives let users hand-roll an HTTPS request today.

### `std.fs`

- Capability-gated: every op takes a `FsCap` carrying an optional
  prefix-allowlist. `FsCap::unrestricted()` skips the check (used by
  trusted CLI entry points).
- `read`, `write`, `exists`, `list_dir` cover the v0.2 surface.
- `list_dir` returns entries in lexicographic order so callers get
  deterministic results.

### `std.time`

- `now(Clock)` returns a monotonic `Instant`.
- `sleep(Clock, dur)` is async (tokio); `sleep_blocking` is the
  synchronous fallback used by the interpreter.
- `Instant.elapsed_since(other)` saturates to `Duration::ZERO` when
  `self < other` (matches `std::time::Instant::checked_duration_since`).

### `std.test`

- Discovery: walks `tests/` recursively, picks every `.sd` file,
  treats every `fn` whose name begins with `test_` as a test.
- Execution: parse → HIR-lower → type+borrow check → SIR-lower →
  invoke via `sdust_sir::interp::run_fn_with_budget` with a 5M-step
  budget.
- Reporter prints `cargo test`-style `ok` / `FAILED` lines plus a
  summary; exit code is non-zero iff any test failed.
- **v0.3 plan**: replace the `test_` prefix convention with a real
  `test fn` syntax + `#[test]` attribute parser change. Out of
  scope for the wave-2 slice (touches sdust-syntax/sdust-hir).
- Ships as a standalone `sdust-test` binary; v0.3 merges it into the
  main `sdust` CLI as `sdust test`.

## Build-time gotcha — workspace race

This slice ran in parallel with two other agents (Wasm CM, DWARF) that
were modifying `crates/sdust-codegen-cranelift/` and
`crates/sdust-codegen-wasm/` simultaneously. While the parallel work
was uncommitted, the workspace build for `sdust-stdlib --features runner`
intermittently broke. The default `--no-default-features` test path
sidesteps the issue by avoiding the driver → codegen chain.

Reproduce the green run with:

```bash
cargo test -p sdust-stdlib --no-default-features
```

Once the parallel agents land their slices, the full
`cargo test -p sdust-stdlib` (runner feature on) goes green too.

## Open follow-ups for v0.3

1. Driver wiring: call `sdust_stdlib::host::install()` from
   `sdust_driver::pipeline::run_file_with_runtime` so `sdust run`
   programs see real `std.*` semantics.
2. `Json::Int(i64)` + `Json::Uint(u64)` variants to preserve precision.
3. `std.tls` native root cert loading (`rustls-native-certs`).
4. `std.http` HTTPS client (`hyper-rustls`) + HTTP/2 server.
5. `std.test` syntax (`test fn` / `#[test]`) — requires parser change.
6. Merge `sdust-test` binary into `sdust test` subcommand.
7. Strategy-B migration: ship `.sd` source files for each `std.*`
   module via `sdust-pkg`.
