# Stardust → Mighty rename compatibility (v0.36 T4)

The v0.7 release renamed the language from **Stardust** to **Mighty**.
v0.7 kept a minimal back-compat surface — most prose changed but a
handful of wire shapes (env-var prefixes, WIT package namespaces, OTLP
span names, registry slug) were left on the legacy `stardust*` spelling
so v0.6-era deployments wouldn't break on upgrade.

v0.36 Track T4 finishes the migration: every surface now emits the
`mty*` / `mighty` / `MTY_*` spelling by default, and the legacy
spellings are accepted as deprecated input. This page is the canonical
contract for the compat layer.

## Categories

T4 split each remaining `stardust` reference into one of four
categories:

| Category | Meaning                                                                           | Disposition         |
|----------|-----------------------------------------------------------------------------------|---------------------|
| A        | Hard rename — prose, comments, doc tables, error messages                         | Renamed             |
| B        | Env vars — primary `MTY_*` + legacy `STARDUST_*` fallback with deprecation warning | Both spellings work |
| C        | Wire surfaces — emit `mty*`, still parse legacy `stardust*`                       | Both spellings work |
| D        | Bench fn names (`stardust_*` in criterion groups)                                 | Left as-is — historical baseline |
| F        | History (CHANGELOG, RELEASE notes, REBRAND_NOTES, dev/history)                    | Left as-is          |
| G        | On-disk directory path (`C:\Users\ihass\stardust` or `~/stardust`)                | Left as-is — user-local |

## Category B: env-var precedence

Every env var the toolchain reads goes through
[`mty_runtime::env_compat::lookup_env`]
(`crates/mty-runtime/src/env_compat.rs`). The precedence:

1. `MTY_<KEY>` if set and non-empty → use it.
2. `STARDUST_<KEY>` if set and non-empty → use it; emit a one-shot
   `mighty: warning: STARDUST_<KEY> is deprecated; use MTY_<KEY> instead`
   on stderr.
3. Otherwise → `None`.

The deprecation warning is per-process, per-key (a `static
AtomicBool` slot per key in `warn_once_flag`); repeated probes during
the same run do not flood stderr.

The renamed vars:

| Legacy             | Primary           | Read by                                    |
|--------------------|-------------------|--------------------------------------------|
| `STARDUST_LINKER`  | `MTY_LINKER`      | Cranelift native backend (`find_linker`)   |
| `STARDUST_OTLP_ENDPOINT` | `MTY_OTLP_ENDPOINT` | Runtime telemetry OTLP exporter      |
| `STARDUST_TRACE`   | `MTY_TRACE`       | Runtime event tracing                      |
| `STARDUST_RUNTIME_THREADS` | `MTY_RUNTIME_THREADS` | Per-worker scheduler count        |
| `STARDUST_CONF_ONLY` | `MTY_CONF_ONLY` | Conformance harness filter                 |
| `STARDUST_CONF_CASE` | `MTY_CONF_CASE` | Conformance single-case picker             |
| `STARDUST_HTTP_MOCK` | `MTY_HTTP_MOCK` | std.http test transport                    |
| `STARDUST_DET_SEED`  | `MTY_DET_SEED`  | Deterministic scheduler seed               |
| `STARDUST_REPLAY_RECORD` | `MTY_REPLAY_RECORD` | Replay recorder toggle             |
| `STARDUST_REPLAY_PLAY`   | `MTY_REPLAY_PLAY`   | Replay player toggle               |
| `STARDUST_RECORD_TRACE`  | `MTY_RECORD_TRACE`  | DAP trace recorder                 |

The Cranelift codegen's `find_linker` open-codes the same precedence
(it can't depend on `mty-runtime`); the test suite covers both
spellings via `find_linker_prefers_mty_over_stardust` and
`find_linker_falls_back_to_stardust` in
`crates/mty-codegen-cranelift/src/object.rs`.

## Category C: wire surfaces

Four wire shapes accept both spellings as input but emit the new one:

| Surface                | Emit                                | Accept                              |
|------------------------|-------------------------------------|-------------------------------------|
| WIT package namespace  | `mty:<pkg>`                         | `mty:<pkg>` + `stardust:<pkg>`      |
| OTLP span names        | `mty.<span>`                        | (caller emits both via `legacy_span_name`) |
| Registry slug          | `mighty-pkg/registry`               | `mighty-pkg/registry` + `stardust-pkg/registry` |
| Cranelift segment      | `b"mighty"`                         | `b"mighty"` + `b"stardust"`         |

**WIT.** `mty-codegen-wasm::wit::PKG_NAMESPACE` is `"mty"`;
`LEGACY_PKG_NAMESPACE` is `"stardust"`. Use `accepted_pkg_namespace`
to recognise either when parsing existing `.wit` files. The
`legacy_stardust_namespace_parses` test fixes the round-trip
contract.

**OTLP.** Runtime spans are now emitted as `mty.turn.start`,
`mty.send`, `mty.shutdown`, etc. Dashboards keyed on the old
`stardust.*` names can still match because every span carries a
`mty.legacy_name` attribute holding the pre-rename spelling.
`legacy_span_name("mty.send")` → `"stardust.send"` is exposed for
consumers that need to compute the legacy attribute themselves.

**Registry.** `mty_pkg::registry::DEFAULT_REGISTRY_SLUG` is
`"mighty-pkg/registry"`. `LEGACY_REGISTRY_SLUG` is
`"stardust-pkg/registry"`. `is_official_registry` accepts both,
so manifests that pinned the legacy slug continue to resolve to the
canonical GitHub-Releases-backed index.

**Cranelift segment.** Object files emit a segment named `b"mighty"`
in the metadata section. The legacy name is still accepted by
`accepted_segment_name` so external inspectors (e.g. `nm`, `objdump`,
`mty inspect --object`) that grep for the old name keep working.

## What the user sees

- A v0.7-style program with `MTY_LINKER=clang` continues to work
  unchanged.
- A v0.7-style program with `STARDUST_LINKER=clang` continues to work
  unchanged, but prints one deprecation line on the first lookup.
- A v0.7-style manifest with `registry = "stardust-pkg/registry"`
  continues to resolve to the official registry.
- A v0.7-style `.wit` file with `package stardust:foo;` continues to
  parse.
- An object inspector that greps `b"stardust"` against a v0.36-built
  binary returns no hits (the segment is now `b"mighty"`); the
  legacy-aware fall-back path is exercised in tests but not emitted
  by the compiler.

## Deprecation policy

The legacy spellings are retained "until v1.2" per the v1.0-RC freeze
contract (`docs/spec/v1.0-rc.md` §2693). v1.2 will:

1. Promote the deprecation warning to a hard error for env vars.
2. Drop the `LEGACY_*` constants from the codegen + pkg crates.
3. Stop accepting `stardust:<pkg>` and `stardust-pkg/registry` as input.
4. Stop emitting `mty.legacy_name` on OTLP spans.

History (Cat F) and the on-disk directory path (Cat G) are unaffected
by the deprecation timeline — they are not user-facing toolchain
surfaces.

## See also

- [`docs/spec/v1.0-rc.md`](../spec/v1.0-rc.md) §2693 — the
  REBRAND_NOTES section enumerating each renamed surface.
- [`dev/history/notes/REBRAND_NOTES.md`](https://github.com/hassard0/Mighty/blob/main/dev/history/notes/REBRAND_NOTES.md)
  — the v0.7 rename log.
- [`dev/history/releases/RELEASE-v0.36.md`](https://github.com/hassard0/Mighty/blob/main/dev/history/releases/RELEASE-v0.36.md)
  — the T4 sweep that finished the migration (ref count: 121 → 45).
- `crates/mty-runtime/src/env_compat.rs` — the precedence helper.
- `crates/mty-codegen-wasm/src/wit.rs` — `accepted_pkg_namespace`.
- `crates/mty-pkg/src/registry.rs` — `is_official_registry`.
- `crates/mty-codegen-cranelift/src/object.rs` — `accepted_segment_name`.
