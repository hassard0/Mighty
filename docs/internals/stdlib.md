# Mighty stdlib (internals)

The Mighty standard library lives in the `mty-stdlib` crate. v0.2
ships **Strategy A**: synthesized prelude bindings in `mty-types`
plus real Rust impls in `mty-stdlib`, hooked into the MtyIR
interpreter through `mty-runtime`'s effect-call sink. v0.3 will
migrate to **Strategy B**: real `.sd` source files shipped as a
bundled package and resolved by `mty-pkg`.

## Strategy A — how it works

```
+-----------+      +-----------------+      +-------------------+
|  user .sd | ---> | mty-types     | ---> | MtyIR EffectOp::    |
|  source   |      | prelude registers|     | GenericCall {     |
|           |      | std.json,        |     |   path, method,   |
|           |      | std.tls, ...     |     |   args }          |
+-----------+      +-----------------+      +-------------------+
                                                      |
                                                      v
                                          +---------------------------+
                                          | mty-runtime::host_std   |
                                          | StdHost::effect_call      |
                                          |  1. sandbox check         |
                                          |  2. forward to installed  |
                                          |     dispatcher fn         |
                                          +---------------------------+
                                                      |
                                                      v
                                          +---------------------------+
                                          | mty-stdlib::host::      |
                                          | dispatch                  |
                                          | match (module, method) -> |
                                          |  real impl                |
                                          +---------------------------+
                                                      |
                          +---------------------------+---------------------------+
                          v                           v                           v
                +-------------------+      +-------------------+      +-----------------+
                | json: serde_json  |      | http: hyper       |      | fs: std::fs     |
                |                   |      | tls: rustls       |      | time: tokio     |
                +-------------------+      +-------------------+      +-----------------+
```

### Why a function pointer?

The runtime can't take a `[dependencies] mty-stdlib` line without
creating a cycle: stdlib needs the runtime (for the `Value` type and
the install hook), and a runtime→stdlib edge would close the loop
through `mty-driver`. We side-step the cycle with a process-wide
`OnceLock<fn(&[String], &str, &[Value]) -> Value>`:

- `sdust_runtime::host_std::install_dispatcher(f)` stores `f` once.
- `sdust_stdlib::host::install()` calls
  `install_dispatcher(sdust_stdlib::host::dispatch)`.

Idempotent — safe to call from every test and once at driver start.
When no dispatcher is registered, `effect_call` returns `Value::Unit`,
matching the slice-7 surface.

### Wiring `mty run` to the real stdlib

For end-user programs that use `use std.json` + `json.parse(s)` to hit
the real parser, the driver must call `sdust_stdlib::host::install()`
during its setup. The hook is in place; the v0.3 driver agent flips
the switch in a one-line change (the v0.2 wave-2 work-area
constraints kept this slice from editing the driver). See
[STDLIB_V0_2_NOTES.md](https://github.com/hassard0/Mighty/blob/main/STDLIB_V0_2_NOTES.md) for the open
follow-ups.

## Module surface

| Module     | Crate module              | Real impl                       |
|------------|---------------------------|---------------------------------|
| `std.json` | `sdust_stdlib::json`      | `serde_json`                    |
| `std.tls`  | `sdust_stdlib::tls`       | `rustls` 0.23 + `tokio-rustls`  |
| `std.http` | `sdust_stdlib::http`      | `hyper` 1.x + `hyper-util`      |
| `std.fs`   | `sdust_stdlib::fs`        | `std::fs` + cap allowlist       |
| `std.time` | `sdust_stdlib::time`      | `std::time` + `tokio::time`     |
| `std.test` | `sdust_stdlib::test`      | MtyIR interpreter (via driver)    |

Reference pages for each surface live under
[`docs/reference/stdlib/`](../reference/stdlib/).

## Crate features

`mty-stdlib` exposes one optional feature:

- `runner` (default-on) — pulls `mty-driver` + `mty-diagnostics`
  so `std.test::run_dir` can compile + execute Mighty test files.
  Downstream crates that only need JSON/TLS/HTTP/FS/time can disable
  it with `default-features = false`.

## v0.3 migration to Strategy B

Strategy B ships `.sd` source files under `crates/mty-stdlib/src/lib/`
(e.g. `json.sd`, `tls.sd`) and bundles them at compile time so the
driver's resolver can `use std.json` without a synthesized prelude
entry. That move needs:

1. `mty-pkg` real module resolution (currently scaffolded).
2. A `bundle.rs`-style include mechanism for shipping `.sd` source
   alongside the binary.
3. Re-targeting each `.sd` file's externs to call the existing Rust
   impls in this crate via the MtyIR extern table.

Tracked in the package-manager design notes
([`docs/internals/package-manager.md`](package-manager.md)).
