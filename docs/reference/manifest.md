# star.toml — Package Manifest

Every Stardust package contains a `star.toml` at its root. The slice 1
loader (in `sdust-driver`) parses two sections: `[package]` and `[deps]`.

The spec describes additional sections (`[build]`, target lists,
profile-specific overrides — see
[spec §5.3](../spec/v0.1.md)). They are reserved and will be wired in
later slices.

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

A table of dependency name → version. Empty by default. Slice 1 does
not resolve dependencies; the loader only parses the section.

```toml
[deps]
std = "0.1"
otel = "0.1"
```

## Reserved / future

The following are accepted by the spec but not by the slice 1 loader:

- `[build]` (target lists, release mode, etc.).
- `[deps.<name>]` tables for path / git / registry sources.
- `[lints]` and other profile-specific configuration.
- `star.lock` — package hash lockfile.

## Errors

Manifest parse errors propagate from the underlying `toml` crate. The
driver wraps them in `ManifestError::Toml` and the I/O failure in
`ManifestError::Io`. The CLI does not currently surface these directly
to end users — `sdust check` operates on single source files, not
packages.
