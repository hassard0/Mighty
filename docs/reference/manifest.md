# star.toml — Package Manifest

Every Stardust package contains a `star.toml` at its root. The slice 1
loader (in `sdust-driver`) parses `[package]` and `[deps]`; v0.2
extends `[deps]` with detailed source specifications and adds an
optional `[build]` section.

Other reserved sections (target lists, profile-specific overrides —
see [spec §5.3](../spec/v0.1.md)) remain unwired.

## Minimal example

```toml
[package]
name = "hello"
version = "0.1.0"
edition = "2026"
profile = "host"

[deps]
```

This is exactly what [`sdust new`](cli/sdust-new.md) emits.

## `[package]`

| Key | Type | Required | Notes |
|---|---|---|---|
| `name` | string | yes | Package identifier. Should match the directory name. |
| `version` | string | yes | Semantic version (`MAJOR.MINOR.PATCH`). |
| `edition` | string | yes | Language edition. v0.1 uses `"2026"`. |
| `profile` | string | no | One of `"host"`, `"web"`, `"edge"`, `"core"`. Defaults to `"host"`. See [spec §2.5](../spec/v0.1.md). |

## `[deps]`

A table of dependency name → source specification. Empty by default.

v0.2's package manager (`sdust pkg`, see
[`docs/reference/cli/sdust-pkg.md`](cli/sdust-pkg.md)) consumes this
section. Slice 1's compiler driver does *not* — it only parses.

Each value can take one of four forms.

### 1. Registry version (the short form)

```toml
[deps]
std = "0.1"
otel = "0.1"
```

Equivalent to a detailed dep with `version = "0.1"` and no other
source. v0.2's registry is a stub; see
[the CLI reference](cli/sdust-pkg.md) for the caveat.

The version string follows the small semver subset described in
[`semver.rs`](../../crates/sdust-pkg/src/semver.rs):

- `"1.2.3"` — caret-equivalent (`^1.2.3`).
- `"=1.2.3"` — exact.
- `"^1.2"` / `"~1.2"` — cargo-style.
- `"0.1"` — partial caret (`^0.1`).
- `"*"` — wildcard.

### 2. Local-path dep

```toml
[deps]
mylib = { path = "../mylib" }
```

The resolver loads the target's `star.toml` to discover its version
and transitive deps. Paths are resolved relative to the manifest's
directory.

### 3. Git dep

```toml
[deps]
foo = { git = "https://github.com/user/foo", rev = "abc123" }
```

`rev` may be any git rev: commit sha, tag, or branch. If omitted,
the default branch HEAD is used. v0.2 does not auto-walk the git
dep's transitive deps post-clone; see
[the resolver caveats](../internals/package-manager.md#known-limitations-post-v02-work).

### 4. Detailed registry dep

```toml
[deps]
serde = { version = "1.0", hash = "sha256:..." }
```

`hash`, when present, is interpreted as a pre-shared pin. The
fetcher will reject any source whose computed sha256 differs.

### Conflict rules

- A dep may declare *either* `path` or `git`, not both.
- A dep must declare at least one of `version` / `path` / `git`.
- The same package name cannot resolve to two different versions
  through different deps (the resolver errors with
  `version conflict`).

## `[build]` *(scaffold)*

Reserved for build-script sandboxing (spec §5.4). v0.2 parses and
records the section; runtime enforcement lands in a later slice.

```toml
[build]
script = "build.sd"
allow_net = ["api.crates.io"]
allow_fs = ["target/"]
```

| Key | Type | Notes |
|---|---|---|
| `script` | string | Path to the build script. |
| `allow_net` | list of strings | Network domains the script may reach. |
| `allow_fs` | list of strings | Filesystem paths the script may touch. |

A manifest carrying `[build]` parses fine even on slice-1 toolchains
*after* v0.2 — the field is optional.

## `star.lock`

Generated alongside `star.toml` by any mutating `sdust pkg` command.
TOML, content-addressed. See
[the package-manager internals](../internals/package-manager.md#lockfile-schema)
for the full schema.

```toml
version = 1

[[package]]
name = "std"
version = "0.1.0"
source = "registry+https://pkg.stardust.dev"
hash = "sha256:..."
dependencies = []
```

Commit `star.lock` alongside `star.toml`: it records the exact bytes
your builds resolved.

## Errors

Manifest parse errors propagate from the underlying `toml` crate. The
driver wraps them in `ManifestError::Toml`; I/O failures in
`ManifestError::Io`; serialise failures in `ManifestError::TomlSer`.
`sdust pkg <subcmd>` surfaces these on stderr and exits non-zero.
