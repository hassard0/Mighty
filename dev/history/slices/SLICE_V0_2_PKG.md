# Slice v0.2 — Package Manager (`mty-pkg`)

Notes for the slice-leader consolidating the v0.2 swarm. This file
documents interpretation calls the `mty-pkg` agent made.

## Scope shipped

- `crates/mty-pkg/` — full crate (lib + 4 integration tests).
- `crates/mty-cli/src/cmd/pkg.rs` — CLI subcommand wired into
  `mty-cli/src/main.rs` as the `Pkg` variant.
- `crates/mty-driver/src/manifest.rs` — extended `Dep` enum +
  `BuildConfig` scaffold + `manifest::save` round-trip.
- Docs:
  - `docs/internals/package-manager.md` (new)
  - `docs/reference/cli/mty-pkg.md` (new)
  - `docs/reference/manifest.md` (extended)

## Interpretation calls

1. **`Manifest.deps` value type promoted from `String` to `Dep`.**
   This is an in-place breaking change to the `mty-driver` public
   `Manifest` shape. The driver's existing tests still pass because
   they only use `.len()` and `.contains_key()`. No other crate
   consumes `.deps` values today.

2. **`[deps]` short form (`foo = "0.1"`) preserved via untagged
   serde.** Bare strings deserialise to `Dep::Version(_)`; tables
   deserialise to `Dep::Detailed(_)`.

3. **Resolver does not auto-walk git deps post-fetch.** v0.2 records
   the git dep at synthesised version `0.0.0` and stops. The walker
   could re-load the cloned `mighty.toml` after fetch, but that mixes
   resolve + fetch concerns and is out of scope here. Flagged as
   post-v0.2 work in `docs/internals/package-manager.md`.

4. **Registry fetcher writes the tarball bytes verbatim**, hashing
   them rather than the extracted tree. v0.2 deliberately doesn't
   pull in `tar` + `flate2`; that's a clean follow-up once the
   registry is live and an actual extraction format is locked in.

5. **`publish` produces a deterministic byte stream with a tar.gz
   extension.** Not actually gzipped or in tar format — the
   extension is forward-looking. Real archive encoding is post-v0.2.

6. **Path-fetcher copies rather than symlinks** for cross-platform
   parity (Windows symlinks need elevation).

7. **Hash format = `sha256:<hex>`** — chosen to match Cargo / common
   ecosystem conventions and to leave room for future algorithms
   without schema churn.

8. **Lockfile schema version `1`.** Bumped on any incompatible change.
   Unknown versions error rather than soft-load.

9. **Build-script sandbox is parse-only.** Manifests carrying
   `[build]` round-trip cleanly; no enforcement yet.

## Tests

`cargo test -p mty-pkg` covers:

- `tests/resolver.rs` — happy path, transitive via path dep, version
  conflict detection.
- `tests/lockfile.rs` — disk roundtrip, hand-written parse.
- `tests/fetch_path.rs` — fetch + hash pin + tamper detection +
  `list` output shape.
- `tests/fetch_git.rs` — clone + checkout rev (`#[ignore]` — needs
  network).

Plus unit tests inside each module covering semver matching, tree
hashing, lockfile serialise, path-source URL parsing, git-source URL
splitting, publish determinism, and command-layer round-trips.

## Cross-crate touch list

| file                                                | nature of change          |
|-----------------------------------------------------|---------------------------|
| `crates/mty-driver/src/manifest.rs`               | extended (additive + Dep enum) |
| `crates/mty-cli/src/main.rs`                      | added `Pkg` variant + dispatch |
| `crates/mty-cli/src/cmd/mod.rs`                   | added `pub mod pkg;`      |
| `crates/mty-cli/Cargo.toml`                       | added `mty-pkg = { path = ".." }` |

## Post-v0.2 follow-ups

- Real tar+flate2 in `fetch::registry::fetch` and `publish::publish`.
- Backtracking resolver + transitive registry crawl once the registry
  exposes an index.
- Git-dep post-fetch transitive walk.
- Build-script sandbox enforcement (spec §5.4).
- Semver pre-release tags + build metadata.
- Workspace / virtual-manifest support.
