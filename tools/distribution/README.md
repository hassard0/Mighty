# Mighty distribution manifests

This directory contains every manifest, formula, and Dockerfile
needed to ship Mighty to the mainstream package managers.

The goal: replace `git clone && cargo install` with one-liners like
`brew install mty`, `scoop install mty`, `winget install mty`,
`docker run mighty-lang/mty`.

The manifests are **source artifacts** — they pin to a specific
Mighty release (currently `v0.30.1`). Each subdirectory's README
explains how to publish that artifact to the corresponding distribution
channel.

## At a glance

| Target       | Path                            | Publish destination                                     |
|--------------|---------------------------------|--------------------------------------------------------|
| Homebrew     | `homebrew/mty.rb`               | `hassard0/homebrew-mighty` tap repo (`Formula/mty.rb`)  |
| Scoop        | `scoop/mty.json`                | `hassard0/scoop-mighty` bucket repo (`bucket/mty.json`) |
| winget       | `winget/manifests/h/hassard/mty/0.31.0/` | PR to `microsoft/winget-pkgs`                   |
| Docker       | `docker/Dockerfile`             | `docker.io/mighty-lang/mty` and/or `ghcr.io/hassard0/mty` |
| Devcontainer | `devcontainer/`                 | `docker.io/mighty-lang/devcontainer`                    |
| mise / asdf  | `mise/plugin-stub.md`           | New repo `hassard0/asdf-mty` (instructions only)        |
| snap         | `snap/snapcraft.yaml`           | Snap Store via `snapcraft upload`                       |

## Release pin

All manifests currently target Mighty **`v0.30.1`** (the latest
released binary set). The release line bump to `0.31.0` cuts new
binaries; at that point update every `version` / `url` / `sha256`
field in lock-step.

Current SHA256 pins (verified at write-time against the release page):

| Asset                                          | SHA256                                                              |
|------------------------------------------------|---------------------------------------------------------------------|
| `mty-x86_64-unknown-linux-gnu.tar.gz`          | `c5bb431ea6d3e57c0952ecde6d9943281d00d513d388ae6f55722e810031c602`  |
| `mty-aarch64-apple-darwin.tar.gz`              | `ed786d66da4211724d42e66d289ea530af1a174c7e40cb1f5c38a6fb7700ab8e`  |
| `mty-x86_64-pc-windows-msvc.zip`               | `0f40621640d7b2298e3463caaeb693eb623496fe21960731ed2a03a1cd9f50bb`  |
| `mty-conformance-kit-v0.30.1.tar.gz`           | `fc35df4bea82a90c4514f0945b0c6502f0e1106c335c2853d96cfb2b9ce51512`  |

Gaps in release.yml (no binaries published yet):
- `x86_64-apple-darwin` (Intel Mac)
- `aarch64-unknown-linux-gnu` (Raspberry Pi / Ampere / Graviton)

Until those land, Homebrew documents Rosetta + source-build for Intel
Mac, and Docker / Snap / Scoop are amd64-only.

## Per-release publish runbook

Run these in order after a fresh release tag.

### 0. Refresh SHAs

```bash
for asset in \
    mty-x86_64-unknown-linux-gnu.tar.gz \
    mty-aarch64-apple-darwin.tar.gz \
    mty-x86_64-pc-windows-msvc.zip ; do
  echo "== $asset =="
  curl -sL "https://github.com/hassard0/Mighty/releases/download/v$VERSION/$asset.sha256"
done
```

Edit every manifest listed below to update `version`, the URLs, and the
SHA256s.

### 1. Homebrew

```bash
cp tools/distribution/homebrew/mty.rb ../homebrew-mighty/Formula/mty.rb
cd ../homebrew-mighty
git add Formula/mty.rb
git commit -m "mty $VERSION"
git push
```

End users: `brew tap hassard0/mighty && brew install mty`.

### 2. Scoop

```bash
# Create the bucket repo once:
gh repo create hassard0/scoop-mighty --public \
  --description "Scoop bucket for the Mighty programming language"

# Each release:
cp tools/distribution/scoop/mty.json ../scoop-mighty/bucket/mty.json
cd ../scoop-mighty
git add bucket/mty.json
git commit -m "mty $VERSION"
git push
```

End users:
```powershell
scoop bucket add mighty https://github.com/hassard0/scoop-mighty
scoop install mty
```

### 3. winget

The manifest set under `winget/manifests/h/hassard/mty/<version>/`
gets submitted as a pull request to
[`microsoft/winget-pkgs`](https://github.com/microsoft/winget-pkgs).

Easiest path is the `wingetcreate` tool from Microsoft:

```powershell
# Authenticate gh as the account submitting the PR:
gh auth login

# Update an existing entry, or `new` for the first submission:
wingetcreate update hassard.mty `
  --version 0.30.1 `
  --urls "https://github.com/hassard0/Mighty/releases/download/v0.30.1/mty-x86_64-pc-windows-msvc.zip" `
  --submit
```

Manual path: fork `microsoft/winget-pkgs`, copy this directory's three
YAML files into the same path in the fork, open a PR. The PR title
should be `New version: hassard.mty version 0.30.1`.

End users (Windows 10+ with the App Installer): `winget install hassard.mty`.

### 4. Docker

```bash
docker build \
  -t mighty-lang/mty:0.30.1 \
  -t mighty-lang/mty:latest \
  tools/distribution/docker

docker login
docker push mighty-lang/mty:0.30.1
docker push mighty-lang/mty:latest

# Also mirror to GHCR:
docker tag mighty-lang/mty:0.30.1 ghcr.io/hassard0/mty:0.30.1
docker tag mighty-lang/mty:latest ghcr.io/hassard0/mty:latest
echo "$GHCR_TOKEN" | docker login ghcr.io -u hassard0 --password-stdin
docker push ghcr.io/hassard0/mty:0.30.1
docker push ghcr.io/hassard0/mty:latest
```

End users:
```bash
docker run --rm -v "$PWD:/work" -w /work mighty-lang/mty:0.30.1 check src/main.mty
```

### 5. Devcontainer

```bash
docker build \
  -t mighty-lang/devcontainer:0.30.1 \
  -t mighty-lang/devcontainer:latest \
  tools/distribution/devcontainer

docker push mighty-lang/devcontainer:0.30.1
docker push mighty-lang/devcontainer:latest
```

End users drop `tools/distribution/devcontainer/devcontainer.json`
into their project's `.devcontainer/` and **Reopen in Container**.

### 6. mise / asdf

A real plugin lives in its own repo. Follow the skeleton in
`mise/plugin-stub.md` to set up `hassard0/asdf-mty`. End users then:

```bash
mise plugin add mty https://github.com/hassard0/asdf-mty
mise install mty@0.30.1
mise use -g mty@0.30.1
```

### 7. Snap (optional)

```bash
# One-time:
sudo snap install snapcraft --classic
snapcraft login

# Build the .snap from the manifest:
cd tools/distribution/snap
snapcraft

# Push + release to the stable channel:
snapcraft upload --release=stable mty_0.30.1_amd64.snap
```

End users: `sudo snap install mty --classic`.

## Tokens / secrets you'll need

| Channel       | Secret                                                  |
|---------------|---------------------------------------------------------|
| Homebrew      | git push access to `hassard0/homebrew-mighty`           |
| Scoop         | git push access to `hassard0/scoop-mighty`              |
| winget        | a GitHub fork of `microsoft/winget-pkgs`                |
| Docker Hub    | `docker login` to the `mighty-lang` Docker Hub org      |
| GHCR          | a GitHub PAT with `write:packages`                      |
| Snap Store    | `snapcraft login` (Ubuntu One)                          |

## v0.32 follow-ups

- Add a release-time workflow that re-templates every manifest with
  the new version + freshly fetched SHAs and opens the publish PRs
  automatically (one job per channel, gated on the release-binaries
  job completing).
- Publish `x86_64-apple-darwin` and `aarch64-unknown-linux-gnu`
  binaries from `release.yml` so the Homebrew formula, Snap, and
  Docker can be multi-arch.
- Spin up `hassard0/asdf-mty` so the mise instructions become a real
  one-liner.
- Submit `mty` to homebrew-core (drops the `brew tap` step).
- Cosign-sign Docker images; publish SBOMs.
