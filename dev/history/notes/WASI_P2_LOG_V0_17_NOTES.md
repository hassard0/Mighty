# v0.17 WASI Preview 2 — `log()` direct lowering + adapter opt-out

Status: shipped this slice.
Owners: codegen-wasm.
Supersedes: the v0.13 `wasi:cli/log` shim documented in
`WASI_P2_V0_13_NOTES.md` (the deprecation note attached to that
shim flagged this slice as the migration target).

## What changed

1. **`log()` / `print()` lower to direct P2 imports.** The v0.13
   slice-8 emitter declared a single
   `(import "wasi:cli/log" "log" (func (param i32 i32)))` per core
   module and called it with `(msg_ptr, msg_len)`. v0.17 drops
   that single import and splices a three-call canonical-ABI
   sequence at every `log()` site:

   ```text
   call $get_stdout                           ;; -> output-stream handle (i32)
   local.tee  $log_handle                     ;; stash the handle
   i32.const  $msg_ptr                        ;; payload bytes
   i32.const  $msg_len
   i32.const  $LOG_RETURN_AREA                ;; result-area for the lift
   call $blocking_write_and_flush             ;; (self, ptr, len, ret) -> ()
   local.get  $log_handle
   call $stream_drop                          ;; release the handle
   ```

   The three imports are:

   - `wasi:cli/stdout@0.2.3#get-stdout`
     (`P2DirectImport::LogStdoutGet`),
   - `wasi:io/streams@0.2.3#[method]output-stream.blocking-write-and-flush`
     (`P2DirectImport::LogStreamWrite`),
   - `wasi:io/streams@0.2.3#[resource-drop]output-stream`
     (`P2DirectImport::LogStreamDrop`).

   They're pre-declared in
   `Emitter::predeclare_p2_direct_imports` so the dispatch arm in
   `emit_call` can splice the sequence without shifting any
   function indices mid-body (same trick we used in v0.16 for the
   fs + http direct lowerings).

   The helper `mty_codegen_wasm::preview2::emit_log_call_sequence`
   is the single source of truth for the instruction order — both
   the in-emitter dispatch and any future external test harness
   call into the same helper so the canonical-ABI sequence stays
   in one place.

2. **The unversioned `wasi:cli/log` shim is gone.** The synthesized
   P2 WIT document no longer declares
   `package wasi:cli; interface log { log: func(msg: string); }`,
   and the parallel push into `wit_parser::Resolve` is also
   removed. Anyone inspecting the emitted WIT will see only the
   versioned `wasi:cli@0.2.3` package (the upstream wasi-cli
   interface set). The `synth_world_imports` list dropped the
   `import wasi:cli/log;` line because the corresponding package
   is no longer in the resolve.

3. **Adapter is opt-in.** `Preview2Options::new(_).embed_adapter`
   now defaults to `None` instead of `Some(AdapterKind::Command)`.
   Mighty programs that touch only the surfaces with direct
   lowerings (`std.random`, `std.time`, `std.fs`, `std.http`,
   `log`) ship adapter-free by default — saving ~50 KB on a
   minimal command component.

   Callers that link wasi-libc-built C crates or otherwise need
   P1→P2 translation opt back in via
   `Preview2Options::with_adapter(Some(AdapterKind::Command))`.
   The vendored adapter binaries stay in
   `crates/mty-codegen-wasm/adapter/` for v0.17; v0.18 drops them
   once every Mighty program is verified adapter-free.

## Canonical-ABI sequence detail

`get-stdout` is a free function:

```wit
package wasi:cli@0.2.3;
interface stdout {
  use wasi:io/streams@0.2.3.{output-stream};
  get-stdout: func() -> output-stream;
}
```

Canonical-ABI core-Wasm shape: `() -> i32`. The returned `i32` is
an `own<output-stream>` handle — the same resource type used by
the rest of `wasi:io/streams`.

`blocking-write-and-flush` is a method on `output-stream`:

```wit
package wasi:io@0.2.3;
interface streams {
  resource output-stream {
    blocking-write-and-flush: func(contents: list<u8>) -> result<_, stream-error>;
  }
}
```

The canonical-ABI lift is:

```text
[method]output-stream.blocking-write-and-flush(
  self_handle:  i32,   ;; borrow<output-stream>
  contents_ptr: i32,
  contents_len: i32,
  ret_area:     i32,   ;; where to land `result<_, stream-error>`
) -> ();
```

The return area receives `(tag: i32, err_handle: i32)` — `tag==0`
means `ok`, `tag==1` means `err` and the second `i32` is an
`own<stream-error>` handle. For `log()` we discard the result
because the slice-8 builtin has no `Result` return; the historical
`wasi:cli/log#log` shim was equally fire-and-forget. The
`LOG_RETURN_AREA` lives at byte offset 8544 in the core module's
linear memory (just past the v0.16 `HTTP_RETURN_AREA`); 16 bytes
of headroom are plenty for the 8-byte result + a future second
slot.

`[resource-drop]output-stream` is the canonical-ABI resource-drop
intrinsic — `(self:i32) -> ()`. Each `get-stdout` call must be
matched with exactly one drop so the host's resource table doesn't
leak entries.

## Resource lifecycle

The handle returned by `get-stdout` is **owned** (`own<output-stream>`),
not borrowed. The canonical ABI requires the holder to either
transfer ownership (none of the calls here do) or drop the handle
when it's no longer needed. The v0.17 lowering does both: it
borrows the handle for the `blocking-write-and-flush` call (which
takes `borrow<output-stream>` and leaves ownership with the caller)
and then drops it before the `log()` site returns.

One handle per `log()` call is the simplest correct lifetime —
re-using a single cached handle across calls would require either
a `Once` initialization scaffold or a thread-local in the core
module, neither of which slice-8 ships. The per-call
acquire+drop is observably equivalent to the historical shim
behaviour and the wasi runtime's `get-stdout` implementation is
cheap (it just returns a pointer into the host's pre-existing
stdout descriptor).

## Adapter opt-out rationale

The wasmtime command-adapter is ~54 KB of Wasm even after
`wit-component`'s tree-shaker prunes unused translation paths. For
a Mighty `hello.mty` that only calls `log("…")`, the v0.16
component spent ~54 KB on adapter glue translating the
`wasi:cli/log#log` import (which the adapter then re-issued as
`fd_write` against `fd=1`). With the v0.17 direct lowering the
adapter is dead weight for any program that doesn't reach for
legacy P1 syscalls.

The `Preview2Options::new` default flip from
`Some(AdapterKind::Command)` to `None` makes adapter-free the
common case. Callers that still need P1 translation — typically
those linking a wasi-libc-built C crate — opt back in via
`with_adapter(Some(AdapterKind::Command))`. The
`tests/preview2_log.rs::explicit_adapter_opt_in_works` test pins
that the opt-in path still works end-to-end.

There's a small back-compat consequence in
`wrap_p2::alias_main_as_start`: that helper only synthesizes the
`_start` export when `matches!(embed_adapter, Some(AdapterKind::Command))`,
because the wasmtime command-adapter is the only consumer of
`_start`. With the v0.17 default-None, Mighty's `main` export
stays the sole entry point. This is the correct behaviour for
host runtimes that drive a Mighty component directly via its
`mighty:<pkg>/main` export.

## What didn't change

- The P1 dispatch path (`EmitWasiPreview::P1`) still wires
  `wasi:cli/log#log` exactly as it did in v0.13. The v0.17 flip
  is P2-only.
- The Web target's `mty:web/log#log` import is untouched —
  v0.17 only addresses the WASI surface.
- The vendored adapter binaries stay in
  `crates/mty-codegen-wasm/adapter/`. Removing them is a v0.18
  follow-up once we've verified no in-tree program needs them.

## v0.18 follow-ups

- Drop the vendored `wasi_snapshot_preview1.{command,reactor,proxy}.wasm`
  binaries from the repo. We can do this once every Mighty
  program (including the self-host sources) builds successfully
  with `embed_adapter = None`. A pre-removal audit should grep
  the workspace for `with_adapter(Some(`-shaped calls.
- Promote the log-write result to a real `Result` return in
  Mighty's stdlib so users can detect ENOSPC / broken-pipe.
  Today the result is discarded — fine for fire-and-forget log
  calls but limits the ergonomics of `print()` in a script.
- Wire an `mty build --wasi-adapter` CLI flag through the driver
  so users can opt the adapter back in without dropping to the
  programmatic API. The flag plumbs `BuildOptions` →
  `Preview2Options::with_adapter(Some(AdapterKind::Command))`.
  v0.17 doesn't add this — the codegen-wasm change stands alone
  and the CLI knob is sibling-owned work.

## Test count delta

- `tests/preview2_log.rs` (new, 8 tests):
  - `log_call_emits_p2_imports`
  - `log_in_component_validates`
  - `default_adapter_is_none`
  - `explicit_adapter_opt_in_works`
  - `log_program_no_adapter_runs_smaller`
  - `p2_log_direct_constants_match_stdlib`
  - `log_p1_path_still_uses_legacy_shim`
  - `empty_main_still_builds_under_p2`
- `tests/preview2.rs` updates:
  - `p2_component_imports_include_wasi_log_shim` →
    `p2_component_no_wasi_cli_log_shim_in_v017` (inverted
    expectation).
  - `log_shim_still_present_with_deprecation_note` →
    `log_shim_removed_in_v017` (inverted expectation).
  - `adapter_embedded_for_p2_command` →
    `adapter_default_none_for_p2` (inverted default).
  - `adapter_changes_component_size` — swapped which side gets
    the adapter (default is now no-adapter).
- `crates/mty-stdlib/src/log.rs` (new, 2 tests):
  - `log_p2_constants_are_canonical`
  - `log_write_line_doesnt_panic`

Net: ~+11 new tests, 4 tests updated to reflect the v0.17
behaviour change.
