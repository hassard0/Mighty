# WASI P2 fs + http direct lowering — v0.16 notes

This note covers the v0.16 slice that **drops the
preview1-adapter dependency for `std.fs` and `std.http`** by emitting
direct P2 imports for those modules. After this slice, the only
adapter-routed call left is `log()` (deferred to v0.17). Builds on
top of the v0.15 emitter wiring for `std.random` + `std.time` and
the v0.14 vendored adapter + WIT surface.

## What ships direct vs. adapter vs. shim

After v0.16, on a `--wasi=p2` build (the default since v0.15):

| Mighty surface | transport | core-module import |
|----------------|-----------|---------------------|
| `std.random.bytes(n)` / `u64()` | **direct** | `wasi:random/random@0.2.3#get-random-bytes` |
| `std.time.now()` / `monotonic_now()` / `resolution()` | **direct** | `wasi:clocks/{wall-clock,monotonic-clock}@0.2.3` |
| `std.fs.open(path)` | **direct (v0.16)** | `wasi:filesystem/types@0.2.3.[method]descriptor.open-at` |
| `std.fs.read_file(path)` | **direct (v0.16)** | `wasi:filesystem/types@0.2.3.[method]descriptor.read-via-stream` |
| `std.fs.write_file(path, data)` | **direct (v0.16)** | `wasi:filesystem/types@0.2.3.[method]descriptor.write-via-stream` |
| `std.fs.stat(path)` | **direct (v0.16)** | `wasi:filesystem/types@0.2.3.[method]descriptor.stat` |
| `std.fs.close(h)` | **direct (v0.16)** | `wasi:filesystem/types@0.2.3.[resource-drop]descriptor` |
| `std.http.get(url)` / `post(url, body)` | **direct (v0.16)** | `wasi:http/types@0.2.3.[constructor]outgoing-request` |
| `std.http.send(req)` | **direct (v0.16)** | `wasi:http/outgoing-handler@0.2.3#handle` |
| `log()` / `print()` | **shim** (deprecated) | `wasi:cli/log#log` (unversioned; v0.17 removes) |

## Canonical-ABI shapes used

WASI 0.2.3 resource types lower to `i32` handles at the core-Wasm
boundary; methods on a `borrow<resource>` take the handle as the
implicit `self` first parameter. Records (e.g. `descriptor-stat`)
are returned via a caller-supplied return-area pointer. The
emitter mints the following core-Wasm signatures for the v0.16
imports — these are normative for WASI 0.2.3 and are unit-tested
against the WIT in `crates/mty-codegen-wasm/wit/wasi-p2/`:

### Filesystem

```text
[method]descriptor.open-at:
  (self:i32, path-flags:i32, path-ptr:i32, path-len:i32,
   open-flags:i32, descriptor-flags:i32, ret-area:i32) -> ()
  // ret-area holds (tag:i32, descriptor-handle | error-code:i32)

[method]descriptor.read-via-stream:
  (self:i32, offset:i64, ret-area:i32) -> ()
  // ret-area holds (tag:i32, input-stream-handle | error-code:i32)

[method]descriptor.write-via-stream:
  (self:i32, offset:i64, ret-area:i32) -> ()
  // ret-area holds (tag:i32, output-stream-handle | error-code:i32)

[method]descriptor.stat:
  (self:i32, ret-area:i32) -> ()
  // ret-area: 80-byte descriptor-stat record (see
  // `preview2::CANONICAL_ABI_DESCRIPTOR_STAT_SIZE`)

[resource-drop]descriptor:
  (self:i32) -> ()
```

### HTTP

```text
[constructor]outgoing-request:
  (headers:i32) -> i32   // returns the new outgoing-request handle

outgoing-handler.handle:
  (req:i32, opt-tag:i32, opt-handle:i32, ret-area:i32) -> ()
  // opt-tag/opt-handle encode the option<request-options>;
  // ret-area holds (tag:i32, future-incoming-response-handle | error-code:i32)

[method]incoming-response.status:
  (self:i32) -> i32      // status-code is a u16

[method]incoming-response.consume:
  (self:i32, ret-area:i32) -> ()
  // ret-area holds (tag:i32, incoming-body-handle:i32)
```

## Resource-handle lifecycle (open → borrow → close)

For v0.16 the emitter dispatch is **conservative on lifecycle**: the
SIR layer hasn't yet lifted preopened-descriptor handles into the
call shape, so the open + drop scaffold around `read_via_stream` is
emitted as placeholder `(handle=0)` arguments. What gets PINNED in
v0.16:

1. The versioned P2 import lands in the import section verbatim —
   `wit-component::ComponentEncoder` resolves it without an adapter
   hop, and a strict P2 host wires it directly.
2. The produced component validates end-to-end (see
   `fs_program_compiles_to_valid_component` in
   `tests/preview2_fs_http.rs`).
3. The canonical-ABI signatures interned for each import are the
   exact shapes WASI 0.2.3 declares — anyone exercising the
   `wasmtime_p2_smoke` feature against the produced component
   will get a type-clean call, not a wrong-arity trap.

What is **deferred to v0.17**:

- Lifting the preopen-descriptor handle through the SIR so
  `std.fs.read_file(path)` actually opens, reads, then drops a
  real descriptor (currently the call splices read-via-stream
  alone with handle=0; the host fails the call at the canonical-ABI
  boundary but the wasm validates).
- Full streaming for HTTP: `future-incoming-response.subscribe` +
  `pollable.block` to await the response, then incremental
  `input-stream.blocking-read` on the body. v0.16 only splices the
  blocking-style entry points.

## Per-call import-index allocation

A core wasm module references imported functions and module-local
functions through a single shared index space (imports first, then
locals). Lazily appending an import while emitting function bodies
shifts every previously-recorded function index — which the v0.15
random/time dispatch was vulnerable to in principle, but
`tests/preview2.rs` only built core modules (not full P2
components) so the breakage stayed latent.

v0.16 closes the trap by walking the SIR up-front in
`Emitter::predeclare_p2_direct_imports` and reserving an import
slot for every `P2DirectImport` the program will need **before**
`declare_fns` runs. The body emitter then re-uses the cached
index. The fix is also what makes the new
`fs_program_compiles_to_valid_component` test pass — without it
the `main` export's func-index pointed at the wrong type after
`read-via-stream` was lazily appended.

## Component-size impact

`std.fs` + `std.http` programs no longer pull the
`wasi_snapshot_preview1#fd_*` / `sock_*` adapter trampolines into
the linked component. The adapter Wasm itself is still embedded
(for `log()`), but `wit-component`'s tree-shaking now strips the
unused fs + http translation paths.

Rough numbers from `cargo test -p mty-codegen-wasm --test preview2_fs_http`:

- Empty program: ~50 KB (adapter-dominated).
- `std.fs.read_file` program with direct lowering: ~50 KB.
- Same program with `with_adapter(None)`: ~3 KB (validates; only
  the direct fs imports are needed since the program doesn't
  touch `log()`).

Once v0.17 drops the `log()` shim, `with_adapter(None)` becomes
the default and the adapter contribution disappears entirely from
fs/http-heavy components.

## Test coverage

New tests in `crates/mty-codegen-wasm/tests/preview2_fs_http.rs`
(12 total — above the spec's 6+):

- `fs_open_emits_direct_p2_import`
- `fs_read_emits_direct_p2_import`
- `fs_write_emits_direct_p2`
- `fs_stat_emits_direct_p2`
- `fs_close_emits_resource_drop_import`
- `http_get_emits_direct_p2_imports`
- `http_post_emits_direct_p2`
- `http_send_emits_outgoing_handler`
- `fs_program_compiles_to_valid_component` (end-to-end + validation)
- `fs_read_under_p1_skips_direct_import` (back-compat guard)
- `http_get_under_p1_skips_direct_import` (back-compat guard)
- `p2_direct_import_pairs_for_fs_http_match_spec` (enum surface)

The existing 24 tests in `tests/preview2.rs` still pass (the
v0.15 dispatch path is unaffected, and the pre-decl pass works
for the v0.15 imports too — `random_bytes_emits_direct_p2_import`
et al. exercise it implicitly now).

The cross-crate constant pin in
`p2_direct_import_names_match_stdlib_constants` was extended to
cover all 9 new variants — drift between the codegen-side
`P2DirectImport::import_pair()` and the stdlib-side
`P2_DIRECT_IMPORT_*` constants is now caught on every CI run.

## v0.17 follow-ups

1. **`log()` direct lowering** — replace the `wasi:cli/log` shim
   with a real `wasi:cli/stdout@0.2.3#get-stdout` +
   `wasi:io/streams@0.2.3#output-stream.blocking-write-and-flush`
   lift. After this lands, `with_adapter(None)` can become the
   default and the adapter becomes opt-in.
2. **Full fs lifecycle** — lift preopen-descriptor handles through
   the SIR so `std.fs.read_file(path)` actually emits the
   open-at → read-via-stream → resource-drop sequence with real
   handle threading.
3. **Full http streaming** — `future-incoming-response.subscribe`
   + `pollable.block` await, plus incremental
   `input-stream.blocking-read` on the body. Today's v0.16
   `std.http.send` lowering splices the blocking-style entry
   points but treats them as one-shot.
4. **Other minor utilities** still going through the adapter:
   `exit`, `environment.get`, `args.get`. Each is a small
   self-contained slice in the v0.17/v0.18 window.
