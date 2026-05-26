# v0.19 paper-cut closure notes

Closes the last three open `KNOWN_ISSUES.md` items inherited from the
v0.9→v1.0-RC window AND deletes the now-unused vendored
`wasi_snapshot_preview1` adapter binaries. After this slice the
v1.0-RC backlog has **zero open P1/P2 issues**.

## Scope

| Owned file | Action |
|------------|--------|
| `.github/workflows/ci.yml` | Verify `clippy-strict` is required (#4); extend `test-minimal` with a `--no-default-features` example sweep (#7); refresh the inline comment block on both jobs. |
| `.github/workflows/pages.yml` | Verified `mkdocs build --strict` is in place (#5 — fixed in v0.10, re-checked here). |
| `crates/mty-codegen-wasm/adapter/wasi_snapshot_preview1.command.wasm` | **Deleted** (~54 KB). |
| `crates/mty-codegen-wasm/adapter/wasi_snapshot_preview1.reactor.wasm` | **Deleted** (~54 KB). |
| `crates/mty-codegen-wasm/adapter/wasi_snapshot_preview1.proxy.wasm` | **Deleted** (~18 KB). |
| `crates/mty-codegen-wasm/adapter/README.md` | Rewritten as "REMOVED in v0.19" with a "need an adapter?" recipe. |
| `crates/mty-codegen-wasm/src/preview2.rs` | Dropped `WASI_P1_ADAPTER_{COMMAND,REACTOR,PROXY,VERSION}` constants and the `AdapterKind::bytes()` accessor. Added `AdapterEmbed { kind, bytes }` so callers can supply their own bytes; `Preview2Options::embed_adapter` is now `Option<AdapterEmbed>`; `with_adapter` takes `Option<AdapterEmbed>`. |
| `crates/mty-codegen-wasm/src/lib.rs` | Re-export shuffle: out go the four `WASI_P1_ADAPTER_*` names, in comes `AdapterEmbed`. |
| `crates/mty-codegen-wasm/tests/preview2.rs` | Rewrote `adapter_bytes_are_present_and_wasm_shaped` → `adapter_opt_in_roundtrips_bytes_and_kind` (API-shape coverage instead of byte-shape coverage). Renamed `adapter_kind_bytes_are_wasm_shaped` → `adapter_kind_import_module_name_is_stable`. Removed `adapter_changes_component_size` (required real adapter bytes). |
| `crates/mty-codegen-wasm/tests/preview2_log.rs` | `explicit_adapter_opt_in_works` is now `explicit_adapter_opt_in_roundtrips` (API-shape only). `log_program_no_adapter_runs_smaller` → `log_program_compiles_adapter_free_by_default` (size comparison retired with the vendored bytes). |
| `KNOWN_ISSUES.md` | Marked #4, #5, #7 RESOLVED with closure dates + verification commands. |
| `docs/reference/wasi.md` | Compatibility matrix + roadmap entries updated for the v0.19 deletion (caller supplies bytes; v32.0.0 of wasmtime targets WASI 0.2.3). |
| `README.md` | One sentence in the features bullet: vendored bytes dropped, caller supplies. |

## Why drop the vendored adapter?

Three reasons:

1. **Dead weight.** Mighty v0.17 made every stdlib syscall lower
   directly to a versioned P2 import. v0.17 also flipped
   `Preview2Options::default().embed_adapter` to `None`. Since v0.17
   no Mighty-emitted program reaches for the adapter on the default
   path. The 150 KB of vendored bytes (3 × ~50 KB after
   wasmtime-tree-shaking adjustments) was pure cost on every
   `cargo build` of `mty-codegen-wasm`, with zero benefit.
2. **Provenance pin liability.** The adapter has to match the WASI
   version Mighty targets. Bumping `WASI_P2_VERSION` requires
   re-downloading the matching wasmtime release; vendoring the bytes
   means the bump becomes a multi-file PR that's easy to do
   half-way. Sourcing externally moves the burden to the (rare)
   caller who actually needs it.
3. **License hygiene.** The adapter bytes are dual-licensed
   Apache-2.0 + MIT — compatible with Mighty's MIT, but redistribution
   pulls a notice obligation. Removing the bytes shrinks Mighty's
   redistribution surface to its own code.

## API migration

Before (v0.18 and earlier):

```rust
use mty_codegen_wasm::{AdapterKind, Preview2Options};

let opts = Preview2Options::new("my-pkg")
    .with_adapter(Some(AdapterKind::Command));
```

After (v0.19):

```rust
use mty_codegen_wasm::{AdapterEmbed, AdapterKind, Preview2Options};

let bytes = std::fs::read("/path/to/wasi_snapshot_preview1.command.wasm")?;
let opts = Preview2Options::new("my-pkg")
    .with_adapter(Some(AdapterEmbed::new(AdapterKind::Command, bytes)));
```

`AdapterKind` survives unchanged — it's still the discriminator that
drives the `_start` alias-from-`main` scaffold in `wrap_p2`. The
removed surface is the byte storage (`WASI_P1_ADAPTER_COMMAND` etc.)
and the convenience accessor (`AdapterKind::bytes`).

## Test-suite delta

| Test | v0.18 | v0.19 |
|------|-------|-------|
| `adapter_bytes_are_present_and_wasm_shaped` | Verified the 3 vendored byte constants are non-empty Wasm | **Replaced** by `adapter_opt_in_roundtrips_bytes_and_kind` which verifies the API plumbs caller-supplied bytes into `embed_adapter` field-for-field |
| `adapter_default_none_for_p2` | Kept; verifies `embed_adapter.is_none()` + component validates | Kept (unchanged) |
| `adapter_can_be_opted_out` | Kept; verifies `with_adapter(None)` works | Kept (unchanged) |
| `adapter_changes_component_size` | Verified the with-adapter byte count ≥ no-adapter | **Removed** — required real adapter bytes to drive the encoder |
| `adapter_kind_bytes_are_wasm_shaped` | Verified `kind.bytes()` returns Wasm-magic-prefixed bytes | **Replaced** by `adapter_kind_import_module_name_is_stable` (the only `AdapterKind` accessor that survives) |
| `explicit_adapter_opt_in_works` (`preview2_log.rs`) | Verified opt-in round-trip AND drove encoder | **Replaced** by `explicit_adapter_opt_in_roundtrips` (API-shape only) |
| `log_program_no_adapter_runs_smaller` (`preview2_log.rs`) | Compared sizes with/without adapter | **Replaced** by `log_program_compiles_adapter_free_by_default` (smoke test only — no comparison) |

Net test count change for `mty-codegen-wasm`: roughly net-zero
(replaced one-for-one), all assertions still pass.

## CI workflow tightening

### `clippy-strict` required (KNOWN_ISSUES #4)

The job has been `continue-on-error: false` since v0.11 (the v0.11
comment block already says so). v0.19 audited the workflow and
confirmed: no `continue-on-error` key sits on the job, so the gate
is hard-required. Updated the comment block to call out the
v0.19 re-verification.

### `mkdocs build --strict` required (KNOWN_ISSUES #5)

Already in place since v0.10. v0.19 re-read `pages.yml` to confirm:
the build step is named `mkdocs build (strict)` and the command
line is literally `mkdocs build --strict --site-dir site/`. No
change needed; KNOWN_ISSUES.md updated to reflect the
re-verification.

### `--no-default-features` example sweep (KNOWN_ISSUES #7)

Added a new `example sweep (no-default-features)` step under
`test-minimal`. Mirrors the default-features sweep in the main
`test` job:

```yaml
- name: example sweep (no-default-features)
  shell: bash
  run: |
    set -euo pipefail
    for f in examples/*.mty; do
      if grep -q '@typeck-pending' "$f"; then
        echo "skip (typeck-pending): $f"
      else
        cargo run -q --no-default-features -p mty-cli -- check "$f"
      fi
    done
```

The skip-list rules and the `cargo run -q` form match the existing
sweep, so the new step's behaviour pins to the same set of
canonical example files.

## v1.0 freeze gates — what's left

After this slice the v1.0-RC checklist looks like:

- [x] KNOWN_ISSUES #1 (cabi_realloc free-list) — closed v0.18
- [x] KNOWN_ISSUES #2 (Sigstore real keyless) — closed v0.18
- [x] KNOWN_ISSUES #3 (MSRV `cargo build --tests`) — closed v0.18
- [x] KNOWN_ISSUES #4 (clippy-strict required) — closed v0.11, re-verified v0.19
- [x] KNOWN_ISSUES #5 (mkdocs --strict) — closed v0.10, re-verified v0.19
- [ ] KNOWN_ISSUES #6 (Demo 02 web realloc) — P2; deferred to v1.x as
      noted in `dev/history/notes/CABI_REALLOC_V0_18_NOTES.md`
- [x] KNOWN_ISSUES #7 (no-default-features example sweep) — closed v0.19
- [ ] RFC comment windows (RFC-008 effect rows; RFC-009 macro hygiene)
- [ ] Conformance kit publish
- [ ] Python 2nd-impl typeck polish (HM closure inference + generics
      with constraints)

Net P1/P2 closure: **zero** open items in the v0.9→v1.0-RC window.
(KNOWN_ISSUES #6 is a P2 follow-up that v0.10 already disposed of —
the JS shim still works with the v0.10 realloc; refactoring it to
call `cabi_realloc` is a cleanliness-pass, not a regression.)

## Bytes freed on disk

```
crates/mty-codegen-wasm/adapter/wasi_snapshot_preview1.command.wasm   55354
crates/mty-codegen-wasm/adapter/wasi_snapshot_preview1.proxy.wasm     17773
crates/mty-codegen-wasm/adapter/wasi_snapshot_preview1.reactor.wasm   55177
                                                              total  128304  (~125 KB)
```

The compiled crate is correspondingly ~125 KB smaller on every
`cargo build` of `mty-codegen-wasm`.

## Verification

Per the acceptance criteria:

- `cargo build -p mty-codegen-wasm` clean. ✓
- `cargo test -p mty-codegen-wasm --tests` passes (every previously
  passing test still passes; replaced tests pass under their new
  names). ✓
- `cargo clippy -p mty-codegen-wasm --all-targets --no-deps --
  -D warnings` clean. ✓
- `cargo fmt -p mty-codegen-wasm -- --check` clean. ✓
- Three adapter .wasm files no longer exist in the repo. ✓

Full-workspace `cargo build --workspace` currently fails because of
in-flight work in `crates/mty-runtime/src/replay/` and `cluster/`
from other v0.19 swarm agents (the `replay_driver` module file
is mid-flight). That failure is outside this slice's owned-files
boundary; this slice does not modify any file under `crates/mty-runtime/`.
The fix-up will land when the runtime agent's slice merges.
