# Package Manager (`sdust-pkg`)

The package manager owns `star.toml` parsing, dependency resolution,
the `star.lock` lockfile, source fetching, and the bundle/publish
pipeline. It is the v0.2 implementation of spec §5.

The CLI surface is documented separately in
[`docs/reference/cli/sdust-pkg.md`](../reference/cli/sdust-pkg.md);
the manifest schema in [`docs/reference/manifest.md`](../reference/manifest.md).
This document covers the *internals*: data shapes, algorithms, and the
boundaries that v0.2 leaves open for later slices.

## Architecture

```
                ┌──────────────┐
                │ star.toml    │
                │ (Manifest)   │
                └──────┬───────┘
                       │ Resolver::resolve
                       ▼
                ┌──────────────┐
                │ star.lock    │
                │ (Lockfile)   │
                └──────┬───────┘
                       │ fetch::fetch_one (dispatch on source kind)
        ┌──────────────┼──────────────────┐
        ▼              ▼                  ▼
  ┌──────────┐   ┌──────────┐      ┌──────────────┐
  │ path     │   │ git      │      │ registry     │
  │ (copy)   │   │ (git2    │      │ (reqwest →   │
  │          │   │  clone)  │      │  pkg.stardust│
  │          │   │          │      │  .dev — STUB)│
  └──────────┘   └──────────┘      └──────────────┘
        │              │                  │
        └──────────────┼──────────────────┘
                       ▼
            ┌──────────────────────────┐
            │ .stardust/pkgs/<name>-<v>│
            │ + sha256 verified vs lock│
            └──────────────────────────┘
```

`sdust pkg <subcmd>` is a thin wrapper. Mutating subcommands (`add` /
`remove` / `update`) round-trip `star.toml`, re-run the resolver, and
rewrite `star.lock`. Read-only subcommands (`list` / `fetch` /
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

## Lockfile schema

TOML, with a schema version we bump on incompatible changes. v0.2 emits
version `1` and rejects anything else.

```toml
version = 1

[[package]]
name = "std"
version = "0.1.0"
source = "registry+https://pkg.stardust.dev"
hash = "sha256:abc..."
dependencies = []
```

The `source` field is `<kind>+<url>` so any future kind drops in
without a schema bump. Recognised kinds in v0.2:

- `registry+<url>` — version pinned in `version`, bytes fetched from
  `<url>/<name>/<version>.tar.gz`.
- `path+file:///abs/path` — bytes copied verbatim from the local FS.
- `git+<url>` or `git+<url>#<rev>` — `git2` clone + optional rev
  checkout.

`hash` is optional in `star.lock` after `pkg add`/`update` (the
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
      sub_manifest = load(sub_dir / "star.toml")
      chosen[name].dependencies = direct_deps(sub_manifest)
      walk(sub_manifest, sub_dir)
```

`resolve_one` is source-kind-aware:

| kind     | version comes from                                  | sub_dir for recursion |
|----------|-----------------------------------------------------|-----------------------|
| path     | the sub-manifest's `package.version`                | the resolved abs path |
| git      | synthesised `0.0.0` (pre-fetch we can't read it)    | none                  |
| registry | requirement *floor* — `^0.1.2` → `0.1.2`            | none                  |

The registry case is a v0.2 stub. The real registry will respond with
an index of available versions; the resolver will then pick the
highest version satisfying the req and look up its dependencies before
walking.

### Known limitations (post-v0.2 work)

1. No backtracking. Two deps that pull in incompatible versions of a
   third error out instead of trying alternate paths.
2. No transitive registry crawl. Registry deps stop the DFS — we don't
   know their deps without an index.
3. Git deps don't post-resolve. After `pkg fetch` clones the rev, we
   should re-walk to discover that git dep's transitive deps. v0.2
   intentionally skips this.
4. Pre-release tags and build metadata are unsupported by the semver
   matcher (see [`semver.rs`](../../crates/sdust-pkg/src/semver.rs)).

## Fetchers

Each fetcher writes into `<repo_root>/.stardust/pkgs/<name>-<version>/`
and returns a `Fetched { root, hash }`. Hashes are sha256 over either
the tree contents (path / git) or the tarball bytes (registry).

- **`fetch::path`** — `copy_dir_recursive` skipping `.git` and
  `target`. Idempotent: wipes the slot first.
- **`fetch::git`** — `git2::Repository::clone` then `revparse_single`
  + `checkout_tree` + `set_head_detached`.
- **`fetch::registry`** — `reqwest::blocking` GET against
  `<registry>/<name>/<version>.tar.gz`. v0.2 writes the tarball
  verbatim into `slot/source.tar.gz` and hashes the bytes; a later
  slice will extract via `tar` + `flate2`. Until the registry comes
  online this fetcher returns
  `FetchError::Registry("could not reach `...` (the Stardust registry is not yet live in v0.2): ...")`.

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

## Publishing (stub)

`publish::publish(repo_root)` walks the package tree (excluding
`.git`, `target`, `.stardust`), concatenates entries into a
deterministic byte buffer (`<plen u32 LE><path><blen u64 LE><body>`),
writes it to `.stardust/publish/<name>-<version>.tar.gz`, and returns
its sha256.

The bytes are *not* actually gzip + tar in v0.2 — the file extension
is forward-looking. A later slice will swap in real tar+flate2 and
add the upload step to the registry. The deterministic byte stream is
enough for hash-based content addressing now.

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

`sdust-pkg` depends on `sdust-driver` purely for the `Manifest` types
re-exported from `sdust_driver::manifest`. Conceptually those types
belong to `sdust-pkg`, but keeping them in the driver preserves the
existing slice-1 loader and avoids ripping up call sites in the
compiler pipeline. A future cleanup can move them into `sdust-pkg`
and have the driver re-export.

`sdust-cli` depends on `sdust-pkg` only through the `commands` module;
the CLI subcommand surface (`PkgCmd`) is a thin adapter.

## File map

- `crates/sdust-pkg/src/lib.rs` — surface re-exports, constants.
- `crates/sdust-pkg/src/semver.rs` — version + requirement matcher.
- `crates/sdust-pkg/src/lockfile.rs` — `Lockfile` / `LockedPackage`.
- `crates/sdust-pkg/src/resolver.rs` — DFS resolver.
- `crates/sdust-pkg/src/fetch/mod.rs` — fetcher dispatch.
- `crates/sdust-pkg/src/fetch/path.rs` — path-source fetcher.
- `crates/sdust-pkg/src/fetch/git.rs` — git-source fetcher.
- `crates/sdust-pkg/src/fetch/registry.rs` — registry stub.
- `crates/sdust-pkg/src/hash.rs` — sha256 helpers.
- `crates/sdust-pkg/src/publish.rs` — bundle stub.
- `crates/sdust-pkg/src/commands.rs` — high-level `pkg add/remove/...`.
- `crates/sdust-cli/src/cmd/pkg.rs` — CLI shim.
