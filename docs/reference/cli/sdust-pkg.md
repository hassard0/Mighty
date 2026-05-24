# `sdust pkg`

Package manager for Stardust. Modifies `star.toml` and `star.lock`,
materialises dependencies under `.stardust/pkgs/`, and bundles
packages for publishing.

```text
USAGE:
    sdust pkg [--manifest-dir <DIR>] <COMMAND> ...

COMMANDS:
    add        Add a dependency to star.toml and update the lockfile
    remove     Remove a dependency
    update     Re-resolve dependencies (optionally just one)
    fetch      Materialise all locked dependencies on disk
    list       Print the resolved dependency tree
    publish    Bundle the current package for publishing (registry not yet live)

GLOBAL OPTIONS:
    --manifest-dir <DIR>   Override the package root (default: cwd)
```

The default package root is the current working directory; pass
`--manifest-dir` to operate on a different one.

## `sdust pkg add`

Add a dependency.

```text
USAGE:
    sdust pkg add <SPEC> [--version <V>] [--path <P> | --git <URL>] [--rev <R>]

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
sdust pkg add std@0.1
sdust pkg add otel --version 0.1

# Local-path dep.
sdust pkg add mylib --path ../mylib

# Git dep pinned to a rev.
sdust pkg add foo --git https://github.com/user/foo --rev abc123
```

After `add`, `star.toml` gains the new key under `[deps]` and
`star.lock` is rewritten. The dep is *not* fetched automatically — run
`sdust pkg fetch` to materialise it.

## `sdust pkg remove`

```text
USAGE:
    sdust pkg remove <NAME>
```

Removes the dep from `star.toml` and re-runs the resolver so
`star.lock` drops orphans.

## `sdust pkg update`

```text
USAGE:
    sdust pkg update [NAME]
```

Re-resolves the manifest end-to-end. With the registry offline in
v0.2 this is mostly useful for path / git deps; for registry deps the
result is the requirement floor either way.

## `sdust pkg fetch`

```text
USAGE:
    sdust pkg fetch
```

For each entry in `star.lock`, runs the appropriate fetcher:

| source kind     | action                                              |
|-----------------|-----------------------------------------------------|
| `path+...`      | Recursive copy into `.stardust/pkgs/<name>-<v>/`    |
| `git+...`       | `git clone` + checkout the optional `#rev`          |
| `registry+...`  | HTTP GET tarball (registry is **not yet live**)     |

After fetching, sha256 of the materialised tree (or tarball bytes for
the registry source) is computed and compared against `LockedPackage.hash`:

- If `hash` was empty (first fetch), it's pinned in `star.lock`.
- If `hash` was set and differs, fetch aborts with a `hash mismatch`
  error.

## `sdust pkg list`

```text
USAGE:
    sdust pkg list
```

Prints the resolved tree:

```text
app v0.1.0
├── mylib v0.3.0 (path)
├── otel v0.1.0 (registry)
│   └── std
└── std v0.1.0 (registry)
```

## `sdust pkg publish`

```text
USAGE:
    sdust pkg publish
```

Produces `.stardust/publish/<name>-<version>.tar.gz` and prints its
sha256. v0.2 stops here — the Stardust registry is not yet live, so
upload is deferred to a later slice. The on-disk bundle is
deterministic (re-running produces a byte-identical file).

## Exit codes

| code | meaning                                        |
|------|------------------------------------------------|
| 0    | success                                        |
| 1    | any `sdust pkg ...` error printed to stderr    |

## Environment

- `RUST_LOG` — not consumed in v0.2; package operations are quiet by
  default. A later slice may add a verbose flag for fetch progress.

## See also

- [Manifest schema](../manifest.md)
- [Package manager internals](../../internals/package-manager.md)
