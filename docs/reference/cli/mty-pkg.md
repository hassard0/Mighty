# `mty pkg`

Package manager for Mighty. Modifies `mighty.toml` and `mighty.lock`,
materialises dependencies under `.mighty/pkgs/`, and bundles
packages for publishing.

```text
USAGE:
    mty pkg [--manifest-dir <DIR>] <COMMAND> ...

COMMANDS:
    add        Add a dependency to mighty.toml and update the lockfile
    remove     Remove a dependency
    update     Re-resolve dependencies (optionally just one)
    fetch      Materialise all locked dependencies on disk
    list       Print the resolved dependency tree
    search     Substring-match cached registry indexes
    info       Show metadata for a published package
    login      Store a GitHub token for a registry
    publish    Bundle the current package and (when authed) upload it

GLOBAL OPTIONS:
    --manifest-dir <DIR>   Override the package root (default: cwd)
```

The default package root is the current working directory; pass
`--manifest-dir` to operate on a different one.

## `mty pkg add`

Add a dependency.

```text
USAGE:
    mty pkg add <SPEC> [--version <V>] [--path <P> | --git <URL>] [--rev <R>]

ARGS:
    <SPEC>   Package name. May be `name@version` shorthand.

OPTIONS:
    --version <V>   Version requirement (alternative to `name@version`).
    --path <P>      Local path source. Implies a detailed dep.
    --git <URL>     Git source. Implies a detailed dep.
    --rev <R>       Git rev / tag / branch (use with --git).
```

### Examples

```sh
# Registry dep (registry is a v0.2 stub — resolver still records it).
mty pkg add std@0.1
mty pkg add otel --version 0.1

# Local-path dep.
mty pkg add mylib --path ../mylib

# Git dep pinned to a rev.
mty pkg add foo --git https://github.com/user/foo --rev abc123
```

After `add`, `mighty.toml` gains the new key under `[deps]` and
`mighty.lock` is rewritten. The dep is *not* fetched automatically — run
`mty pkg fetch` to materialise it.

## `mty pkg remove`

```text
USAGE:
    mty pkg remove <NAME>
```

Removes the dep from `mighty.toml` and re-runs the resolver so
`mighty.lock` drops orphans.

## `mty pkg update`

```text
USAGE:
    mty pkg update [NAME] [--refresh]
```

Re-resolves the manifest end-to-end. With `--refresh`, every
configured registry's cached index is re-pulled from GitHub Releases
**before** resolution runs — pick this when you want to catch newly
published versions. Without `--refresh`, resolution is offline-only
(the local cache is the source of truth, TTL 1 hour).

## `mty pkg fetch`

```text
USAGE:
    mty pkg fetch
```

For each entry in `mighty.lock`, runs the appropriate fetcher:

| source kind     | action                                              |
|-----------------|-----------------------------------------------------|
| `path+...`      | Recursive copy into `.mighty/pkgs/<name>-<v>/`    |
| `git+...`       | `git clone` + checkout the optional `#rev`          |
| `registry+...`  | HTTP GET tarball (registry is **not yet live**)     |

After fetching, sha256 of the materialised tree (or tarball bytes for
the registry source) is computed and compared against `LockedPackage.hash`:

- If `hash` was empty (first fetch), it's pinned in `mighty.lock`.
- If `hash` was set and differs, fetch aborts with a `hash mismatch`
  error.

## `mty pkg list`

```text
USAGE:
    mty pkg list
```

Prints the resolved tree:

```text
app v0.1.0
├── mylib v0.3.0 (path)
├── otel v0.1.0 (registry)
│   └── std
└── std v0.1.0 (registry)
```

## `mty pkg search`

```text
USAGE:
    mty pkg search <QUERY>
```

Substring-matches `<QUERY>` against the name and version of every
release in the cached registry indexes. With no cached indexes,
prints a "no results" message that hints at
`mty pkg update --refresh`.

Example:

```text
$ mty pkg search otel
otel@0.1.0    [mighty-pkg/registry]
otel-rt@0.3.5 [mighty-pkg/registry]
```

## `mty pkg info`

```text
USAGE:
    mty pkg info <NAME>[@<VERSION>]
```

Shows release metadata (URLs, sha256-sidecar URL, manifest body) for
a published package. Without `@<version>`, picks the highest known
version across the configured registries.

Example:

```text
$ mty pkg info otel@0.1.0
otel@0.1.0    [mighty-pkg/registry]
  release : https://github.com/mighty-pkg/registry/releases/tag/otel-0.1.0
  tarball : https://github.com/mighty-pkg/registry/releases/download/otel-0.1.0/otel-0.1.0.tar.gz
  manifest:
    [package]
    name = "otel"
    version = "0.1.0"
    edition = "2026"
```

## `mty pkg login`

```text
USAGE:
    SDUST_PKG_LOGIN_TOKEN=<token> mty pkg login [REGISTRY]
```

Stores a GitHub token for a registry slug in
`~/.config/mty/auth.toml` (mode `0600` on Unix). The token must be
passed via the `SDUST_PKG_LOGIN_TOKEN` env-var — v0.4 disables
interactive prompts so the same path drives both human use and CI.

`REGISTRY` defaults to the `[registry].default` configured in
`mighty.toml`, falling back to the official Mighty registry slug.

Auth-token lookup precedence at fetch / publish time:

1. `~/.config/mty/auth.toml` `[tokens]` entry for the slug.
2. `GITHUB_TOKEN` env-var.

See [`docs/reference/registry.md`](../registry.md#authentication) for
the auth-store schema and security tradeoffs.

## `mty pkg publish`

```text
USAGE:
    mty pkg publish
```

Builds a deterministic `tar.gz` of the package contents plus a
`.tar.gz.sha256` sidecar under `.mighty/publish/`. When a token is
available for the configured default registry (per the lookup
precedence above), it then:

1. Creates a GitHub Release on `<owner>/<repo>` tagged
   `<name>-<version>`.
2. Uploads the `tar.gz` + sidecar as release assets.
3. Prints the release page URL.

Without a token, the local artefacts are still written and the
output explains how to set `GITHUB_TOKEN` or `mty pkg login` plus
the manual drag-and-drop fallback. Exit code is 0 in the
"bundle-only, no upload" case — the bundle is a useful artefact in
its own right (e.g. for air-gapped distribution).

## Exit codes

| code | meaning                                        |
|------|------------------------------------------------|
| 0    | success                                        |
| 1    | any `mty pkg ...` error printed to stderr    |

## Environment

- `GITHUB_TOKEN` — used (a) to lift the GitHub Releases API rate
  limit from 60/hr to 5000/hr for all registries, and (b) as a
  fallback when no per-slug token is present in
  `~/.config/mty/auth.toml`. Required scopes: `repo` for private
  registries or `publish`.
- `SDUST_PKG_LOGIN_TOKEN` — token-source for the non-interactive
  `mty pkg login` flow.
- `RUST_LOG` — not consumed in v0.4; package operations are quiet by
  default.

## See also

- [Manifest schema](../manifest.md)
- [Registry concept + storage convention](../registry.md)
- [Package manager internals](../../internals/package-manager.md)
