# Stardust Registry

The Stardust package registry is a thin convention layered on **GitHub
Releases**. There is no central server: a registry is just a GitHub
repository (`<owner>/<repo>`) whose Releases host one tag per published
`(package-name, version)`. This document covers:

- [Storage convention](#storage-convention)
- [Index discovery and caching](#index-discovery-and-caching)
- [Resolution semantics](#resolution-semantics)
- [Multiple registries](#multiple-registries)
- [Authentication](#authentication)
- [Publishing](#publishing)
- [Hosting your own registry](#hosting-your-own-registry)
- [Security model](#security-model)
- [Roadmap](#roadmap)

> **Status.** The registry transport shipped in v0.4. The *official*
> registry (`stardust-pkg/registry`) is not yet created on GitHub — that
> belongs to the v0.5 cloud control plane. Until then, configure
> `[registry].default` to point at any GitHub repo that follows the
> storage convention below.

## Storage convention

Each published version is one **GitHub Release** on the registry
repository with:

| Field        | Value                                                       |
|--------------|-------------------------------------------------------------|
| Tag          | `<package-name>-<version>` (e.g. `otel-0.1.0`)              |
| Asset 1      | `<package-name>-<version>.tar.gz` — gzipped tar of the source |
| Asset 2      | `<package-name>-<version>.tar.gz.sha256` — sidecar (hex hash + filename) |
| Release body | Verbatim `star.toml` from the published version             |

Package names may contain dashes; the tag parser splits on the **last
dash before a digit**, so `my-lib-1.2.3` parses as `(my-lib, 1.2.3)`.

The tarball expands into a single top-level `<name>-<version>/`
directory (matching the cargo / npm convention) with deterministic
ordering, mtime, uid, gid → identical bytes on every rebuild.

The sidecar file is a single line in the `sha256sum -b` shape:

```
<hex-digest>  <name>-<version>.tar.gz
```

`sdust pkg fetch` accepts either the bare hex digest, the
`sha256:<hex>` form, or the full sha256sum line.

## Index discovery and caching

`sdust-pkg` discovers releases via the standard GitHub Releases REST
API:

```
GET https://api.github.com/repos/<owner>/<repo>/releases?per_page=100&page=N
```

Pagination is automatic (capped at 50 pages / 5000 releases per
registry). Each release whose `tag_name` parses into
`(name, version)` is catalogued; everything else is ignored.

The parsed catalogue is cached locally per package, under:

```
<package-root>/.stardust/registry/<owner>__<repo>/index.json
```

- **TTL**: 1 hour. After expiry the next operation that needs the
  index revalidates via `If-Modified-Since` (304 → bump the cache
  timestamp, keep the existing list).
- **Force refresh**: `sdust pkg update --refresh` re-pulls every
  configured registry's index.

Resolution (`sdust pkg add`, `sdust pkg update`) is intentionally
**offline-first**: it only reads the on-disk cache, never the
network. This keeps `add` fast and reproducible. If you want the
latest available versions, `update --refresh` first.

## Resolution semantics

For each registry dep:

1. Walk the configured registries (default first, then each `extras`
   entry in order).
2. In each registry's cached index, find every release matching
   `name` and whose version satisfies the semver requirement.
3. Pick the **highest matching version**. Stop scanning further
   registries — first-listed wins on duplicates.
4. If no registry has a match (no cache, empty index, package
   missing), fall back to synthesising the version from the
   requirement floor (`^0.3.2` → `0.3.2`). The lockfile still pins
   the default registry's slug; `sdust pkg fetch` will then surface a
   clear "release not found" error if the package truly isn't there.

The lockfile records the chosen registry slug:

```toml
[[package]]
name = "otel"
version = "0.1.0"
source = "registry+gh://stardust-pkg/registry"
hash = "sha256:..."
```

## Multiple registries

A package's `star.toml` may opt into additional registries:

```toml
[registry]
default = "stardust-pkg/registry"            # the official one
extras = ["myorg/private-stardust-pkgs"]     # additional registries
```

Lookup order is `default`, then `extras` in declared order. On
duplicate `(name, version)` across registries, **first hit wins**.

Omitting `[registry]` is equivalent to:

```toml
[registry]
default = "stardust-pkg/registry"
extras = []
```

## Authentication

Two layers exist:

1. **API rate limits.** Even for public registries, an unauthed
   GitHub token gets you 60 requests/hour; a personal access token
   (PAT) raises that to 5000/hour. Set `GITHUB_TOKEN` in your
   environment to use it across every registry.

2. **Private registries + publish.** Per-registry tokens are stored
   at:

   ```
   ~/.config/sdust/auth.toml
   ```

   (Windows: `%APPDATA%\sdust\auth.toml`). The file shape:

   ```toml
   [tokens]
   "myorg/private-stardust-pkgs" = "ghp_..."
   "stardust-pkg/registry"       = "ghp_..."
   ```

   On Unix, `auth.toml` is written with mode `0600`. The same
   storage model as `gh` CLI's `~/.config/gh/hosts.yml`.

   Configure tokens with `sdust pkg login`:

   ```sh
   # token must be provided via env-var (v0.4 disables interactive prompts)
   SDUST_PKG_LOGIN_TOKEN=ghp_xxxxx sdust pkg login myorg/private-stardust-pkgs
   ```

Lookup precedence: the per-slug token in `auth.toml` first, then
`GITHUB_TOKEN`.

The required GitHub-token scopes:

| Operation         | Minimum scope                                          |
|-------------------|--------------------------------------------------------|
| Read public idx   | none (unauthed) or any token (better rate limit)       |
| Read private repo | `repo`                                                  |
| Publish           | `repo` (creates releases + uploads assets)              |

## Publishing

```sh
# Bundle + (when authed) upload as a GitHub Release.
sdust pkg publish
```

`publish` always writes the local artefacts:

- `.stardust/publish/<name>-<version>.tar.gz`
- `.stardust/publish/<name>-<version>.tar.gz.sha256`

When a token is available for the configured default registry, it
proceeds to:

1. `POST /repos/<owner>/<repo>/releases` with the tag, manifest body,
   `draft: false`, `prerelease: false`.
2. Upload the tarball asset (Content-Type: `application/gzip`).
3. Upload the sha256 sidecar (Content-Type: `text/plain`).

On success, the new release URL is printed. On 401/403/404, the
artefacts are still on disk and the user can drag them onto the
release page manually.

### Bundle exclusions

Files under these top-level paths are **excluded** from the tarball:

- `.git/`
- `target/`
- `.stardust/` (your local cache lives here)

There is no `package.include`/`exclude` field yet — that's tracked as
a post-v0.5 enhancement.

## Hosting your own registry

1. Create a public or private GitHub repository, e.g.
   `myorg/our-stardust-pkgs`. The repo's README + LICENSE are
   optional; the registry only cares about Releases.
2. For each package you want to publish, follow the [storage
   convention](#storage-convention). The easiest path is `sdust pkg
   publish` from inside the package directory, after a `sdust pkg
   login myorg/our-stardust-pkgs`.
3. Point consumers at your registry:

   ```toml
   [registry]
   default = "myorg/our-stardust-pkgs"
   ```

That's it. No custom server, no DNS, no infra. GitHub handles
storage, integrity (release tags + commit signatures), and bandwidth.

## Security model

| Threat                              | Mitigation                                          |
|-------------------------------------|-----------------------------------------------------|
| Tarball tampered in transit         | sha256 sidecar verified pre-extract                 |
| Tarball tampered post-fetch         | `star.lock` pins sha256; subsequent fetches fail   |
| Malicious tarball path-traversal    | Extraction rejects `..`, root, and drive-prefix    |
| Compromised registry repo           | Use a private/owned registry; pin lock hashes      |
| Leaked publish token                | Per-slug tokens; revoke individually in GitHub UI  |
| Compromised maintainer account      | (post-v0.5: signed releases + co-maintainer review) |

The plaintext token file is the same tradeoff `gh` CLI and `cargo
login` make. On Unix the file is mode `0600`. On Windows we rely on
NTFS user-profile ACLs.

## Roadmap

Beyond v0.4:

- **Yanked packages.** A `yanked: true` field in the release body
  with a clear consumer-side warning. Lockfile pin still works, but
  `pkg add` skips yanked versions.
- **Security advisories.** A separate `security/` directory in the
  registry repo with one Markdown file per advisory; `sdust pkg
  audit` cross-references it.
- **Signed releases.** GitHub release artefacts have ETags but no
  built-in signing today; a sigstore + cosign pipeline would
  pre-sign the tarball and embed the signature next to the sidecar.
- **Mirror support.** A read-only mirror configured in addition to
  the default lets large orgs cache hot packages.
- **Package include/exclude.** `[package].include` and `.exclude`
  globs to refine which files get tarred.

The cloud control plane will eventually create
`hassard0/stardust-pkg-registry` (or similar) and seed it with the
stdlib — that's tracked as v0.5 work.
