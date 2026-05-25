# Package Manager (`mty-pkg`)

The package manager owns `mighty.toml` parsing, dependency resolution,
the `mighty.lock` lockfile, source fetching, and the bundle/publish
pipeline. It is the v0.2/v0.4 implementation of spec §5.

The CLI surface is documented separately in
[`docs/reference/cli/mty-pkg.md`](../reference/cli/mty-pkg.md);
the manifest schema in [`docs/reference/manifest.md`](../reference/manifest.md);
the registry concept + storage convention in
[`docs/reference/registry.md`](../reference/registry.md).
This document covers the *internals*: data shapes, algorithms, and
the boundaries that v0.4 leaves open for later slices.

> **v0.4 added:** real GitHub-Releases registry transport (replacing
> the v0.2 `pkg.mighty.dev` stub), per-package `[registry]` config,
> on-disk index cache, auth store, real tar.gz publish bundles, and
> the `search` / `info` / `login` CLI subcommands.

## Architecture

```
                ┌──────────────┐
                │ mighty.toml    │
                │ (Manifest)   │
                └──────┬───────┘
                       │ Resolver::resolve
                       ▼
                ┌──────────────┐
                │ mighty.lock    │
                │ (Lockfile)   │
                └──────┬───────┘
                       │ fetch::fetch_one (dispatch on source kind)
        ┌──────────────┼──────────────────┐
        ▼              ▼                  ▼
  ┌──────────┐   ┌──────────┐      ┌──────────────────┐
  │ path     │   │ git      │      │ registry (gh://) │
  │ (copy)   │   │ (git2    │      │ GitHub Releases  │
  │          │   │  clone)  │      │ + index cache +  │
  │          │   │          │      │ sha256 verify +  │
  │          │   │          │      │ tar.gz extract   │
  └──────────┘   └──────────┘      └──────────────────┘
        │              │                  │
        └──────────────┼──────────────────┘
                       ▼
            ┌──────────────────────────┐
            │ .mighty/pkgs/<name>-<v>│
            │ + sha256 verified vs lock│
            └──────────────────────────┘
```

`mty pkg <subcmd>` is a thin wrapper. Mutating subcommands (`add` /
`remove` / `update`) round-trip `mighty.toml`, re-run the resolver, and
rewrite `mighty.lock`. Read-only subcommands (`list` / `fetch` /
`publish`) operate against the existing lockfile.

## Manifest schema (extended)

The slice-1 `Manifest` already had `[package]` + a flat
`BTreeMap<String, String>` for `[deps]`. v0.2 keeps that shape for the
common case but switches the value type to a `Dep` enum:

```rust
pub enum Dep {
    Version(String),                // foo = "0.1"
    Detailed(DetailedDep),          // foo = { version = "..", path = "..", ... }
}

pub struct DetailedDep {
    pub version: Option<String>,
    pub path:    Option<String>,
    pub git:     Option<String>,
    pub rev:     Option<String>,
    pub hash:    Option<String>,
}
```

`Dep` uses serde's `#[serde(untagged)]` representation so both bare
strings and detailed tables parse with the same key.

A new top-level `[build]` section is *parsed and recorded* but not
enforced; see [§Build sandbox scaffold](#build-sandbox-scaffold) below.

## Registry config (`[registry]`)

v0.4 adds an optional `[registry]` section to `mighty.toml`:

```toml
[registry]
default = "mighty-pkg/registry"          # default registry slug
extras = ["myorg/private-mighty-pkgs"]   # additional registries
```

This section lives **outside** the `Manifest` struct in `mty-driver`
— `mty-pkg` re-parses `mighty.toml` independently via
[`registry::load_registry_config`](https://github.com/hassard0/Mighty/blob/main/crates/mty-pkg/src/registry.rs).
That keeps the slice from touching the driver crate.

Lookup is offline-first: resolution walks the **cached** index, never
the network. `mty pkg update --refresh` re-pulls every configured
registry's index before re-resolving.

## Lockfile schema

TOML, with a schema version we bump on incompatible changes. v0.2 emits
version `1` and rejects anything else.

```toml
version = 1

[[package]]
name = "std"
version = "0.1.0"
source = "registry+https://pkg.mighty.dev"
hash = "sha256:abc..."
dependencies = []
```

The `source` field is `<kind>+<url>` so any future kind drops in
without a schema bump. Recognised kinds:

- `registry+gh://<owner>/<repo>` — v0.4. Version pinned in `version`,
  bytes fetched from the GitHub release tagged `<name>-<version>`.
- `registry+<url>` — v0.2 legacy. Parsed but no longer extracted —
  `pkg fetch` returns a clear "re-run pkg update to migrate" error.
- `path+file:///abs/path` — bytes copied verbatim from the local FS.
- `git+<url>` or `git+<url>#<rev>` — `git2` clone + optional rev
  checkout.

`hash` is optional in `mighty.lock` after `pkg add`/`update` (the
registry stub doesn't know the bytes yet). It becomes mandatory after
`pkg fetch` — that call pins the actual sha256 of what landed on disk.

`dependencies` lists the *names* of transitive deps already present in
this same lockfile. v0.2 records these for `pkg list` rendering and
future audit tools; the resolver does not currently rely on them for
re-validation.

## Resolver algorithm

Greedy DFS:

```
resolve(root_manifest):
  chosen = {}      # name -> ChosenDep
  visited = set()  # names whose dependents we've already walked
  walk(root_manifest, root_dir)
  emit lockfile from chosen

walk(manifest, manifest_dir):
  for (name, dep) in manifest.deps:
    validate_dep(name, dep)              # not both path+git, not empty
    (version, source, sub_dir?) = resolve_one(name, dep)
    if name in chosen:
      if chosen[name].version != version: ERROR VersionConflict
      continue
    chosen[name] = ChosenDep(version, source, [])
    if first_visit(name) and sub_dir is not None:
      sub_manifest = load(sub_dir / "mighty.toml")
      chosen[name].dependencies = direct_deps(sub_manifest)
      walk(sub_manifest, sub_dir)
```

`resolve_one` is source-kind-aware:

| kind     | version comes from                                  | sub_dir for recursion |
|----------|-----------------------------------------------------|-----------------------|
| path     | the sub-manifest's `package.version`                | the resolved abs path |
| git      | synthesised `0.0.0` (pre-fetch we can't read it)    | none                  |
| registry | highest version in cached index satisfying req      | none                  |

v0.4 wires the registry case to the cached
[`RegistryIndex`](https://github.com/hassard0/Mighty/blob/main/crates/mty-pkg/src/registry.rs). For each
registry slug (default first, extras after) we look up `(name, req)`
and stop at the first match. When no cached index matches (no index
at all, package missing), the resolver falls back to the v0.2
requirement-floor synthesis so offline development still works — and
the lockfile is still pinned with the default registry's slug, ready
for a `pkg update --refresh` later.

### Known limitations (post-v0.2 work)

1. No backtracking. Two deps that pull in incompatible versions of a
   third error out instead of trying alternate paths.
2. No transitive registry crawl. Registry deps stop the DFS — we don't
   know their deps without an index.
3. Git deps don't post-resolve. After `pkg fetch` clones the rev, we
   should re-walk to discover that git dep's transitive deps. v0.2
   intentionally skips this.
4. Pre-release tags and build metadata are unsupported by the semver
   matcher (see [`semver.rs`](https://github.com/hassard0/Mighty/blob/main/crates/mty-pkg/src/semver.rs)).

## Fetchers

Each fetcher writes into `<repo_root>/.mighty/pkgs/<name>-<version>/`
and returns a `Fetched { root, hash }`. Hashes are sha256 over either
the tree contents (path / git) or the tarball bytes (registry).

- **`fetch::path`** — `copy_dir_recursive` skipping `.git` and
  `target`. Idempotent: wipes the slot first.
- **`fetch::git`** — `git2::Repository::clone` then `revparse_single`
  + `checkout_tree` + `set_head_detached`.
- **`fetch::registry`** — v0.4 GitHub Releases backend.
  1. Resolve the registry slug from
     `registry+gh://<owner>/<repo>` in the lockfile.
  2. Ensure a cached index exists for that slug
     (`.mighty/registry/<owner>__<repo>/index.json`, 1-hour TTL).
  3. Look up the `(name, version)` release; download its `.tar.gz`
     asset + the `.tar.gz.sha256` sidecar.
  4. Verify sidecar hash, then verify against the lockfile's pinned
     hash if present.
  5. Extract through `tar::Archive` over a `GzDecoder`. Entries with
     `..`, root, or drive-prefix components are rejected (path
     traversal defence).

Hash verification fires on every fetch: if `LockedPackage::hash` is
populated, the fetcher errors on mismatch. On first fetch (hash empty)
the resolver pins whatever it computed.

## Hashing

`hash::hash_tree(root)` walks all regular files under `root`, sorts
the relative paths, and feeds
`<rel-path>\0<bytes>\0` per entry into a single `Sha256`. `.git` and
`target` are excluded. Cross-platform determinism comes from
normalising relative paths to forward slashes.

`hash::hash_bytes(bytes)` is the obvious sha256-of-bytes.

Both helpers return `sha256:<hex>` (lowercase, no separators).

## Publishing

`publish::bundle(repo_root)` walks the package tree (excluding
`.git`, `target`, `.mighty`), feeds the sorted entries into a
`tar::Builder` with deterministic header settings (mode 0644, mtime
0, uid/gid 0, GNU header), and writes the gzipped output to
`.mighty/publish/<name>-<version>.tar.gz`. A sidecar file
`<name>-<version>.tar.gz.sha256` is written next to it with the
standard `sha256sum -b` shape:

```
<hex>  <name>-<version>.tar.gz
```

All entries live under a `<name>-<version>/` top-level directory so
extraction produces a tidy single root. Re-running `bundle` yields
byte-identical artefacts.

`publish::upload(slug, outcome)` is the optional second step. Given a
GitHub token (per-slug entry in `~/.config/mty/auth.toml` or
`GITHUB_TOKEN`), it:

1. `POST /repos/<owner>/<repo>/releases` with tag, manifest body,
   draft=false.
2. Uploads `<name>-<version>.tar.gz` as a release asset.
3. Uploads `<name>-<version>.tar.gz.sha256` as a release asset.

The CLI calls `bundle` unconditionally and then `upload` only when a
token is available, falling back to a clear "set GITHUB_TOKEN" hint
that includes the local file paths so users can drag-and-drop onto
the release page manually.

## Build sandbox scaffold

Spec §5.4 mandates sandboxed build scripts. v0.2 only parses + stores
the `[build]` section so manifests carrying it don't break:

```toml
[build]
script = "build.sd"
allow_net = ["api.crates.io"]
allow_fs = ["target/"]
```

The fields are exposed via `Manifest.build: Option<BuildConfig>`. No
runtime enforcement is implemented — that's deferred to the slice that
introduces the build-script execution path.

## Cross-crate boundary

`mty-pkg` depends on `mty-driver` purely for the `Manifest` types
re-exported from `sdust_driver::manifest`. Conceptually those types
belong to `mty-pkg`, but keeping them in the driver preserves the
existing slice-1 loader and avoids ripping up call sites in the
compiler pipeline. A future cleanup can move them into `mty-pkg`
and have the driver re-export.

`mty-cli` depends on `mty-pkg` only through the `commands` module;
the CLI subcommand surface (`PkgCmd`) is a thin adapter.

## File map

- `crates/mty-pkg/src/lib.rs` — surface re-exports, constants.
- `crates/mty-pkg/src/semver.rs` — version + requirement matcher.
- `crates/mty-pkg/src/lockfile.rs` — `Lockfile` / `LockedPackage`.
- `crates/mty-pkg/src/resolver.rs` — DFS resolver.
- `crates/mty-pkg/src/fetch/mod.rs` — fetcher dispatch.
- `crates/mty-pkg/src/fetch/path.rs` — path-source fetcher.
- `crates/mty-pkg/src/fetch/git.rs` — git-source fetcher.
- `crates/mty-pkg/src/fetch/registry.rs` — GitHub Releases fetcher
  + index cache management (v0.4).
- `crates/mty-pkg/src/registry.rs` — `[registry]` config, index
  schema, auth store, slug parsing (v0.4).
- `crates/mty-pkg/src/hash.rs` — sha256 helpers.
- `crates/mty-pkg/src/publish.rs` — tar.gz bundle + GitHub Releases
  upload (v0.4).
- `crates/mty-pkg/src/commands.rs` — high-level `pkg add/remove/...`.
- `crates/mty-cli/src/cmd/pkg.rs` — CLI shim.
