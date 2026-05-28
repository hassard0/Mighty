# asdf-mty

An [asdf](https://asdf-vm.com) / [mise](https://mise.jdx.dev) plugin
for the [Mighty programming language](https://github.com/hassard0/Mighty).

> This directory is the **plugin skeleton** as shipped from the
> Mighty source tree at `tools/distribution/asdf-mty/`. It must be
> published as a **separate GitHub repository** (`hassard0/asdf-mty`)
> for `asdf plugin add` / `mise plugin add` to consume it. See
> [Publishing](#publishing) below.

## Install (end users)

### mise

```bash
mise plugin add mty https://github.com/hassard0/asdf-mty
mise install mty@0.32.0
mise use -g mty@0.32.0

mty --version
```

After the plugin registry PR (see [Publishing](#publishing) step 3)
lands, the URL can be dropped:

```bash
mise install mty@0.32.0
```

### asdf

```bash
asdf plugin add mty https://github.com/hassard0/asdf-mty
asdf install mty 0.32.0
asdf global mty 0.32.0

mty --version
```

## Supported platforms

The plugin downloads pre-built binaries from the
[Mighty GitHub Releases](https://github.com/hassard0/Mighty/releases).
v0.32.0+ ships five `(os, arch)` combinations:

| OS      | Architecture       | Asset                                       |
|---------|--------------------|---------------------------------------------|
| Linux   | x86_64             | `mty-x86_64-unknown-linux-gnu.tar.gz`       |
| Linux   | aarch64 / arm64    | `mty-aarch64-unknown-linux-gnu.tar.gz`      |
| macOS   | Apple Silicon      | `mty-aarch64-apple-darwin.tar.gz`           |
| macOS   | Intel              | `mty-x86_64-apple-darwin.tar.gz`            |
| Windows | x86_64 (msys/git-bash) | `mty-x86_64-pc-windows-msvc.zip`        |

Older Mighty releases (v0.31 and earlier) ship only the first,
third, and fifth rows. Trying to `mise install mty@0.31.0` on a
Raspberry Pi will fail with a clear "unsupported platform" message
from `lib/utils.bash`.

## Plugin layout

```
asdf-mty/
├── README.md              # this file
├── LICENSE                # MIT (matches Mighty's license)
├── bin/
│   ├── list-all           # `mise ls-remote mty` source
│   ├── download           # populates $ASDF_DOWNLOAD_PATH
│   └── install            # extracts into $ASDF_INSTALL_PATH/bin
└── lib/
    └── utils.bash         # asset_for_platform / sha verify helpers
```

The plugin honors the `ASDF_INSTALL_VERSION`, `ASDF_INSTALL_PATH`,
and `ASDF_DOWNLOAD_PATH` contracts documented at
<https://asdf-vm.com/plugins/create.html>.

## Publishing

This skeleton must be extracted into its own GitHub repo before it
can be consumed by `mise plugin add` / `asdf plugin add`.

### 1. Create the repo

```bash
gh repo create hassard0/asdf-mty --public \
  --description "asdf/mise plugin for the Mighty programming language"
```

### 2. Push the skeleton

From the Mighty source tree:

```bash
# Copy the skeleton out
cp -r tools/distribution/asdf-mty /tmp/asdf-mty
cd /tmp/asdf-mty

# Add an MIT LICENSE matching Mighty's
curl -fsSL \
  https://raw.githubusercontent.com/hassard0/Mighty/main/LICENSE \
  -o LICENSE

git init
git add .
git commit -m "asdf-mty 0.1.0 (initial skeleton)"
git branch -M main
git remote add origin git@github.com:hassard0/asdf-mty.git
git push -u origin main
```

### 3. (Optional) Register with mise's plugin registry

`mise` ships an index that maps short names to plugin URLs. After
registering, users `mise install mty@X.Y.Z` with no URL.

```bash
# Fork + clone:
gh repo fork jdx/mise --clone --remote
cd mise
git checkout -b add-mty

# Edit registry.toml — add the `mty` entry under the right alphabetical slot.
# See https://github.com/jdx/mise/blob/main/registry.toml for the exact schema.

git add registry.toml
git commit -m "registry: add mty"
git push -u origin add-mty
gh pr create --title "registry: add mty" --body "..."
```

### 4. Keep the plugin in sync

The plugin skeleton in `Mighty/tools/distribution/asdf-mty/` is
the source of truth. When the plugin needs an update (new
platform, sha verification tweak, etc.), edit the files **here**
and re-push to `hassard0/asdf-mty`:

```bash
# From the Mighty source tree, after a plugin edit:
rsync -av tools/distribution/asdf-mty/ /tmp/asdf-mty/
cd /tmp/asdf-mty
git add .
git commit -m "asdf-mty 0.X.Y: <what changed>"
git push
```

A future track may automate this with a GitHub Action in the
Mighty repo that pushes plugin changes to `hassard0/asdf-mty` on
every change under `tools/distribution/asdf-mty/`.

## Testing locally

```bash
export ASDF_INSTALL_TYPE=version
export ASDF_INSTALL_VERSION=0.32.0
export ASDF_DOWNLOAD_PATH="$(mktemp -d)"
export ASDF_INSTALL_PATH="$(mktemp -d)"

./bin/list-all              # should print "... 0.30.1 0.31.0 0.32.0"
./bin/download              # populates $ASDF_DOWNLOAD_PATH
./bin/install               # extracts + runs `mty --version`

"$ASDF_INSTALL_PATH/bin/mty" --version
```

## License

MIT — matches the upstream Mighty repository.
