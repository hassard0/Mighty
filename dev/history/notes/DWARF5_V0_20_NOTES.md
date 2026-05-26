# DWARF v5 + per-instruction line program — v0.20 notes

Post-v1.0 roadmap slice. Adds an opt-in DWARF v5 emission path on top
of the existing v4 default in `mty-debuginfo`, dispatched from
`mty-codegen-cranelift` at the existing `attach_dwarf_sections` site.

## Scope

| Phase | File(s) | Status |
|-------|---------|--------|
| v5 builder | `crates/mty-debuginfo/src/dwarf5.rs` (new, ~280 LOC) | done |
| Re-export | `crates/mty-debuginfo/src/lib.rs` | done — `pub use dwarf5::Dwarf5Builder` |
| Codegen dispatch | `crates/mty-codegen-cranelift/src/debug.rs` | done — `build_dwarf_dispatch` + `dwarf5_enabled` (env `MTY_DWARF5=1`) |
| Codegen call-site | `crates/mty-codegen-cranelift/src/object.rs` | done — single-line switch from `build_dwarf_for` → `build_dwarf_dispatch` |
| Tests | `crates/mty-debuginfo/tests/dwarf5.rs` (new) | done — 5 tests (target was 4+) |
| Docs | `docs/internals/debug-info.md` | done — new DWARF v5 section with opt-in instructions, diffs vs v4, tooling matrix, binary-size data |
| Notes | this file | done |

## What v5 actually buys us today

The honest answer: in single-CU workloads, **v5 is slightly larger than
v4** (+3.2% in a 16-fn × 32-row × 4-local synthetic) because:

- The new `.debug_line_str` table has fixed overhead (24 bytes in our
  measurement) regardless of how many strings reference it.
- The v5 `.debug_line` header is a few bytes longer to describe its
  format (file/dir entry shape).

The v5 wins only kick in when:

1. **Many CUs share path strings.** `.debug_line_str` is per-output, so
   N CUs that all live under `/repo/src/` pay for the path once. We
   currently emit one CU per `mty build`, so this win is dormant until
   we either split per-module or merge cross-package builds.
2. **Per-instruction granularity exceeds per-block.** Both the v4 and
   v5 builders emit one row per `FunctionDebugInfo::line_table` entry
   today. The v5 *capacity* for per-instruction rows is wired up
   (`add_function` writes one row per entry, defensively skipping
   non-monotonic addresses), but `function_debug_info` in
   `mty-codegen-cranelift/src/debug.rs` still produces the
   conservative 2-entry table inherited from v0.2. The next slice
   plumbs cranelift's `MachSrcLoc` map all the way through; *then* v5's
   denser opcode-table-encoded rows will compress more tightly than the
   equivalent v4 stream because v5 uses 0-based file indices (one fewer
   ULEB128 byte per row in the common case).

The header-magic, indirect-string-table, and round-trip tests all pass
today — so when the per-instruction MachSrcLoc plumbing lands, the v5
path is a one-env-var flip.

## Implementation notes

### Why an env var instead of a Cargo feature

Considered both. Picked `MTY_DWARF5=1` because:

- The v5 builder lives in `mty-debuginfo` but the user-visible toggle
  is at `mty build` time. A Cargo feature would need to be plumbed
  through `mty-debuginfo` → `mty-codegen-cranelift` → `mty-driver` →
  `mty-cli` to land at the user, and feature unification across the
  workspace would mean tests that rebuild with the feature on would
  invalidate caches for the v4 path.
- A runtime env var lets you A/B the same binary by re-running the
  build, which is the actual debug workflow.
- The cost is one stat per `cargo build` (`std::env::var`), which is
  invisible against any cranelift codegen.

### The `FileId(0)` re-add-file trick

`gimli::write::FileId::new` is `pub(crate)`, so there's no public way
to construct a `FileId(0)` directly. For v5, `LineProgram::new`
*automatically* inserts the comp_file at index 0 — but doesn't return
its id. Trick: `add_file` is idempotent on the `(file, directory)` key,
so calling `add_file` a second time with the same LineString reference
returns the existing `FileId(0)`. Verified by walking the v5 output
through `gimli::read` and confirming `DW_TAG_subprogram`'s
`DW_AT_decl_file` resolves to the right file.

### Defensive monotonic-address handling

`gimli::write::LineProgram::generate_row` has a debug_assert that
address_offsets within a sequence are monotonically increasing. The v4
builder doesn't guard this — a malformed `FunctionDebugInfo` would
panic in debug builds. The v5 builder silently skips out-of-order
entries instead (covered by `dwarf5::tests::drops_out_of_order_rows`).
We didn't backport this to v4 because it would change observable v4
behavior for downstream code; v5 is a fresh slate.

## Verification

```
cargo build -p mty-debuginfo                  → clean
cargo build -p mty-codegen-cranelift          → clean (default v4)
cargo test -p mty-debuginfo --test dwarf5     → 5 passed
cargo test -p mty-debuginfo                   → 22 passed (15 unit + 5 dwarf5 + 2 roundtrip)
cargo test -p mty-codegen-cranelift --lib debug:: → 4 passed (incl. new build_dwarf5_emits_indirect_str_section)
cargo clippy -p mty-debuginfo -p mty-codegen-cranelift --all-targets -- -D warnings → clean
cargo fmt -p mty-debuginfo -p mty-codegen-cranelift -- --check → clean
```

The v4 round-trip tests (`tests/dwarf_roundtrip.rs`) still pass
unmodified, confirming back-compat.

## Follow-ups (not in this slice)

1. **Plumb cranelift `MachSrcLoc` through `define_function`** so
   `function_debug_info` produces a per-instruction `line_table`. This
   is the single biggest unlock for v5 to actually shrink the binary
   (denser rows → better opcode-table compression) and for live
   source-stepping precision to match Rust's `rustc -g`.
2. **`.debug_loclists` per-local from cranelift slot offsets.** Same
   gap as v4. Wire up `Location::Expression(DW_OP_fbreg + offset)`
   once cranelift exposes the slot map.
3. **Cross-CU string sharing via `.debug_line_str`.** Only meaningful
   once we emit multiple CUs per artifact (package-level granularity).
4. **`Address::Symbol` references for `low_pc`/`high_pc`** — same
   deferred item as v4, blocks live-stepping against the linked
   binary's runtime addresses.

## File count + diff size

| Change | LOC |
|--------|----:|
| `dwarf5.rs` (new) | ~330 |
| `lib.rs` (re-export) | +2 |
| `debug.rs` (dispatch + v5 helper) | +40 |
| `object.rs` (1-line switch) | +1, -3 |
| `tests/dwarf5.rs` (new) | ~200 |
| `docs/internals/debug-info.md` | +90 |
| this notes file | ~150 |

Per the v0.20 swarm rules: only `mty-debuginfo/*` and
`mty-codegen-cranelift/{debug.rs, object.rs}` were touched in the
source tree; no other crates modified.
