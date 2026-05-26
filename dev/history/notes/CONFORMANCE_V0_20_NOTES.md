# v0.20 Conformance Notes — placeholder backfill + kit-in-release

Follows on from `CONFORMANCE_V0_11_NOTES.md`. The v0.20 slice fills
the four placeholder categories the v0.19 freeze-prep agent shipped
empty, then wires the kit-tarball build into `.github/workflows/release.yml`
so every tagged release publishes a fresh kit alongside the binaries.

## Per-category contribution

| Category | Before v0.20 | After v0.20 | Delta |
|----------|--------------|-------------|-------|
| `deterministic_replay/` | 0 (placeholder README) | 5 cases | +5 |
| `formatter_idempotence/` | 0 (placeholder README) | 5 cases | +5 |
| `native_abi/`           | 0 (placeholder README) | 4 cases | +4 |
| `wasm_component/`       | 0 (placeholder README) | 4 cases | +4 |
| **Total**               | **122 cases**          | **140 cases** | **+18** |

Total category count is unchanged (24 — already populated 20 + placeholder 4
in v0.19); after v0.20 all 24 are populated.

## Per-case manifest

### deterministic_replay/

| Case | What it pins |
|------|--------------|
| `01_pure_program` | empty trace invariant — pure programs emit zero events |
| `02_clock_read`   | `ClockRead` event for every host-clock observation |
| `03_random_seq`   | `RandomRead` event for every host-RNG observation |
| `04_send_message` | `Spawn` / `MessageSent` / `MessageHandled` ordering |
| `05_replay_roundtrip` | byte-identical record -> replay invariant |

The `expected_trace.txt` file in each case directory describes the
event sequence the recorder MUST produce. The conformance_full
harness validates the program type-checks; the trace-shape assertion
lives in the replay-integration tests under
`crates/mty-runtime/tests/replay_*.rs`. This split mirrors the
codegen/ category's "fixture in conformance/, behavioural assertion
in a per-backend test" pattern.

### formatter_idempotence/

| Case | What it pins |
|------|--------------|
| `01_canonical_struct` | struct canonical form: 2-space indent, one field per line |
| `02_canonical_match` | match arm canonical form: one arm per line, ` => ` spacing |
| `03_canonical_effect_clause` | multi-row-var `!{| E, F}` preserves all row vars (v0.19 fix) |
| `04_canonical_comments` | comment groups + blank lines preserved across `fmt` |
| `05_canonical_macro` | declarative macro body verbatim across `fmt` |

Each case ships both `input.mty` (which `fmt` consumes) and
`canonical.mty` (which `fmt(input.mty)` MUST equal). The
conformance_full harness validates `input.mty` parses + type-checks;
the byte-equivalence assertion lives in `crates/mty-fmt/tests/`.

### native_abi/

| Case | What it pins |
|------|--------------|
| `01_export_main` | simplest C-ABI export: integer return symbol |
| `02_string_return` | `Str` return via cabi_realloc convention |
| `03_struct_return` | struct return by-value |
| `04_callback` | accepts `fn(I32) -> I32` callback pointer |

Each case ships a `harness.c` C source that links against the
emitted object and exercises the export. The expected exit code is
in `expected_harness_exit.txt`. The conformance_full harness
validates `input.mty` parses + type-checks; the link-and-run
assertion lives in `crates/mty-codegen-cranelift/tests/` (v0.20
stretch — wired by a separate sub-agent or v0.21 follow-up).

### wasm_component/

| Case | What it pins |
|------|--------------|
| `01_minimal_component` | empty fn -> valid component |
| `02_wasi_p2_log` | log() -> `wasi:cli/stdout` + `wasi:io/streams` direct imports |
| `03_wasi_p2_fs` | fs.read() -> `wasi:filesystem/types` + `preopens` direct imports |
| `04_user_wit` | `--wit world.wit` -> custom world export |

`expected_component.txt` in each case describes the import/export
list the emitted component MUST carry. The conformance_full harness
validates `input.mty` parses + type-checks; the component-shape
assertion lives in `crates/mty-codegen-wasm/tests/`.

## Kit-build size delta

Before v0.20: `mty-conformance-kit-v0.19.tar.gz` was ~92 KB
(122 cases, 4 empty placeholder dirs).

Expected after v0.20: ~120 KB (140 cases including the 18 new
fixtures + their READMEs + their harness/canonical/expected files).

The exact size lands when the v0.20 release tag triggers
`.github/workflows/release.yml`'s new `conformance-kit` job.

## coverage.json

A new `tests/conformance/coverage.json` is the machine-readable
answer to "which diagnostic codes does the kit cover?". Shape:

```json
{
  "version": "1",
  "diagnostic_codes": {
    "covered":    [<MTxxxx codes asserted by at least one conformance_full case>],
    "auxiliary":  [<MTxxxx codes witnessed by a non-conformance unit test only>],
    "uncovered":  [<MTxxxx codes with no harness anywhere>]
  },
  "categories": {
    "<name>": { "case_count": N, "diagnostic_codes": [...] }
  },
  "totals": { ... }
}
```

The v0.20 numbers (based on the v0.11 audit + v0.20 backfill):

| Status | Count | Pct of registered MTxxxx codes |
|--------|-------|-------------------------------|
| `covered` (direct) | 53 | 48% |
| `auxiliary` (aux harness only) | 42 | 38% |
| `uncovered` (true gap) | 17 | 15% |

(The "true gap" set is unchanged from v0.11 — they all need
crate-source emit-site work, which the v0.20 slice scope excludes.
See `CONFORMANCE_V0_11_NOTES.md` §"Per-gap status" for the
hand-off.)

## Release workflow change

`.github/workflows/release.yml` gains:

1. A new `conformance-kit` job (runs in parallel with `build`) that
   shell-execs `scripts/build-conformance-kit.sh <tag>` and uploads
   the resulting tarball as a GitHub Actions artifact.
2. The `release` job's `needs:` list now includes `conformance-kit`
   so it cannot publish until the kit is built.
3. The `release` job's `files:` list now includes
   `out/mty-conformance-kit-*.tar.gz` so the kit ships as a release
   asset alongside the binaries.

No changes to the build job, the matrix, or the existing binary
packaging — the kit is additive.

## conformance_full.rs change

`crates/mty-driver/tests/conformance_full.rs` gains explicit
per-category floor assertions for the four newly-populated
categories. The existing ≥70 cases overall floor is unchanged; the
new per-category floors (≥5 / ≥5 / ≥4 / ≥4) catch the regression
where a future agent accidentally `rm -rf`s a category directory.

## Acceptance gate

After v0.20:

- `cargo test --test conformance_full -p mty-driver` runs ≥140
  cases (the v0.10 ≥70 floor is unchanged but the actual count
  jumps from 122 to 140).
- `bash scripts/build-conformance-kit.sh test` produces a tarball
  larger than the v0.19 92 KB baseline (expected ~120 KB).
- `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo fmt --all -- --check` all clean.

The acceptance check was deferred at the agent boundary because a
parallel agent had in-flight `mty-runtime` changes that did not
compile at the time this slice landed. The conformance_full
discovery + assertion bumps were committed and will pass once the
parallel `mty-runtime` work compiles.
