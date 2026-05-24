# Registry v0.4 — Interpretation Notes

Notes on judgement calls + open questions from the v0.4 registry
slice. The spec told us "real package registry stub via GitHub
Releases"; this file records the decisions we made implementing it.

## What shipped

- New `crates/mty-pkg/src/registry.rs` — `[registry]` config,
  `RegistryIndex`, `AuthStore`, slug + tag parsing.
- Rewrite of `crates/mty-pkg/src/fetch/registry.rs` — GitHub
  Releases REST client, on-disk index cache with 1-hour TTL +
  `If-Modified-Since`, sha256 sidecar verification, gzipped tar
  extraction with path-traversal guard.
- Rewrite of `crates/mty-pkg/src/publish.rs` — real `tar.gz` +
  sidecar bundles plus optional GitHub Releases upload.
- Resolver wired to the cached index; falls back to the v0.2
  requirement-floor synthesis when no index is available.
- New CLI subcommands: `search`, `info`, `login`. Existing
  `add` / `remove` / `update` / `fetch` / `list` / `publish` work
  unchanged from the user's POV (richer behaviour underneath).
- Five new integration test files (28 → 64 active tests +
  2 `#[ignore]` network tests).
- Three docs files updated (`docs/internals/package-manager.md`,
  `docs/reference/cli/mty-pkg.md`) plus one new
  (`docs/reference/registry.md`).

## Interpretation calls

### 1. Source URL scheme — `registry+gh://<owner>/<repo>`

The spec didn't pin a syntax. The v0.2 lockfile used
`registry+https://pkg.mighty.dev`; we wanted a shape that:

- Clearly names the backend (GitHub Releases — not just "HTTP").
- Encodes the registry slug compactly enough that a human reading
  `mighty.lock` can identify the source repo at a glance.
- Coexists with the legacy form without confusing the parser.

Chose `registry+gh://<owner>/<repo>`. The fetcher rejects the
legacy `registry+https://...` form with a clear "re-run
`mty pkg update` to migrate" error rather than silently treating
it as `gh://`. Adding a true mirror/HTTP backend later would land
as `registry+https://...` again, this time with an actual handler.

### 2. `[registry]` lives outside `Manifest`

The `Manifest` struct lives in `mty-driver`. The slice rules
prohibit touching the driver, so `mty-pkg` re-parses `mighty.toml`
for the `[registry]` table only when it needs the config. This
keeps blast radius small and avoids a driver-version churn just
for a config block, at the cost of one extra TOML parse per
package operation.

A future cleanup can move `Manifest` itself into `mty-pkg` (the
internals doc already flags this) and consolidate.

### 3. Offline-first resolution

`mty pkg add` / `update` never hits the network — they read the
on-disk cache only. The user opts into a network refresh with
`mty pkg update --refresh`. Rationale: a flaky network shouldn't
break `add`, and `add` should always be deterministic across
invocations on the same cache.

Side effect: on a fresh checkout with no cache, `add foo` will
synthesise the requirement floor for `foo`'s version (because the
index it would consult doesn't exist yet). The user's expected
follow-up is `mty pkg update --refresh && mty pkg fetch`, which
revalidates and pins.

### 4. Token storage = plaintext at `~/.config/mty/auth.toml`

The user spec explicitly asked us to document this tradeoff. We
copy the model `gh` CLI uses (`~/.config/gh/hosts.yml`) and
`cargo` uses (`~/.cargo/credentials.toml`). On Unix the file is
mode `0600`; on Windows we rely on the user-profile ACLs that
already protect `%APPDATA%`.

Future hardening: pluggable secret store (Keychain on macOS,
Credential Manager on Windows, libsecret on Linux). Tracked as
post-v0.5.

### 5. `pkg login` is non-interactive

We considered shelling out to a TUI password prompt, but the
agent-friendly path is "set an env-var, run a command". Plus, the
agent has no PTY here. So `mty pkg login` consumes
`SDUST_PKG_LOGIN_TOKEN` and persists it; a future slice can layer
an interactive prompt on top without changing the storage shape.

### 6. Fallback path on resolver miss

When a registry dep has no match in any index (and the resolver
can't reach the network because it's offline-first), we synthesise
the requirement floor and pin the *default* registry's slug.
Alternative was a hard error.

The float wins: it keeps `add` from breaking on a fresh checkout,
the lockfile still parses, and `pkg fetch` surfaces a clear
"release not found in registry" error if the package really
doesn't exist. The hard-error path would have forced every fresh
checkout to be online — too aggressive.

### 7. Determinism: `tar` header pinning, gzip default level

We pin mtime=0, uid=0, gid=0, mode=0644, GNU header, and feed a
sorted file list. `flate2::Compression::default()` (level 6) is
deterministic for the same input, so the resulting `.tar.gz` is
byte-identical on every rebuild. Verified by the
`bundle_re_run_produces_identical_sha256` and
`bundle_is_deterministic` tests.

### 8. Path-traversal guard

The extractor rejects entries whose paths contain `..`, `RootDir`,
or `Prefix` components. This blocks the obvious "zip slip"-style
attacks. We also set `Archive::set_overwrite(true)` so re-fetches
overwrite the slot, but the slot is always wiped first anyway.

### 9. `pkg publish` exit code on no-token

Currently exits 0 with a clear "upload skipped" message. We
considered exiting non-zero to make CI loud, but the bundle on
disk is itself useful (air-gapped distribution, manual upload).
Hard-erroring would penalise that path. The user can drive the
CI policy with their own grep on the output if they need to.

### 10. Skipped: actually creating `mighty-pkg/registry` on GitHub

The spec explicitly told us not to. The default-slug constant
points at `mighty-pkg/registry` so the v0.5 cloud control plane
can create it without code changes. Until then, any fetch against
the default produces a clean 404 from GitHub which surfaces as
"registry `mighty-pkg/registry` not found on GitHub".

The `#[ignore]`d network smoke test (`registry_fetch.rs`) points
at `octocat/Hello-World` instead so it exercises real transport
without depending on a not-yet-existing repo.

## Post-v0.5 work flagged

- Create `hassard0/stardust-pkg-registry` (or similar slug) and
  seed it with the stdlib.
- Move `Manifest` into `mty-pkg`, leave a re-export in
  `mty-driver`.
- `[package].include` / `.exclude` globs for bundle contents.
- Yanked-version support (release-body marker + consumer warning).
- Security advisory cross-referencing (`mty pkg audit`).
- Signed releases via sigstore/cosign.
- Pluggable secret store (Keychain / Credential Manager / libsecret).
- Interactive `pkg login` (post-TUI).
- Real HTTP/registry-mirror backend (`registry+https://`).
- `pkg search` could rank by download count once that data is
  available (GitHub API exposes per-asset download counts).

## Files touched

```
crates/mty-pkg/Cargo.toml                       # +tar, +flate2, +dirs, +serde_json
crates/mty-pkg/src/lib.rs                       # registry module + re-exports
crates/mty-pkg/src/registry.rs                  # NEW
crates/mty-pkg/src/fetch/registry.rs            # rewrite
crates/mty-pkg/src/publish.rs                   # rewrite (bundle + upload)
crates/mty-pkg/src/resolver.rs                  # registry-index aware
crates/mty-pkg/src/commands.rs                  # +search, +info, +login, +refresh_indexes
crates/mty-pkg/tests/registry_index_parse.rs    # NEW
crates/mty-pkg/tests/registry_fetch.rs          # NEW (#[ignore] network)
crates/mty-pkg/tests/publish_bundle.rs          # NEW
crates/mty-pkg/tests/multi_registry_resolve.rs  # NEW
crates/mty-pkg/tests/auth_token_load.rs         # NEW
crates/mty-cli/src/cmd/pkg.rs                   # +Search,+Info,+Login + Update --refresh
docs/internals/package-manager.md                 # v0.4 section + diagram update
docs/reference/cli/mty-pkg.md                   # search/info/login/publish/update docs
docs/reference/registry.md                        # NEW
REGISTRY_V0_4_NOTES.md                            # NEW (this file)
```
