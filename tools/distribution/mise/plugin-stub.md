# mise / asdf plugin for Mighty

`mise` (and its predecessor `asdf`) expect plugins to live in their
own repos so the tool can `git clone` them. A real plugin therefore
ships from `hassard0/asdf-mty`, not from inside the Mighty source
tree.

This file is a placeholder + instructions for setting that repo up.
Once the plugin repo exists, end users install Mighty via:

```bash
mise plugin add mty https://github.com/hassard0/asdf-mty
mise install mty@0.30.1
mise use -g mty@0.30.1
```

## Plugin repo skeleton (`hassard0/asdf-mty`)

```
asdf-mty/
├── README.md
├── LICENSE                 # MIT, matching Mighty
├── bin/
│   ├── list-all            # prints all available versions, newline-separated
│   ├── download            # downloads the right tarball for the platform
│   └── install             # extracts the tarball into $ASDF_INSTALL_PATH/bin
└── lib/
    └── utils.bash          # platform detection + URL building
```

### `bin/list-all`

```bash
#!/usr/bin/env bash
set -euo pipefail

curl -fsSL \
  "https://api.github.com/repos/hassard0/Mighty/releases?per_page=100" \
  | grep -oE '"tag_name": *"v[0-9]+\.[0-9]+\.[0-9]+"' \
  | sed -E 's/.*"v([^"]+)".*/\1/' \
  | sort -V \
  | tr '\n' ' '
```

### `bin/download`

```bash
#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "$0")/../lib/utils.bash"

VERSION="$ASDF_INSTALL_VERSION"
DOWNLOAD_PATH="$ASDF_DOWNLOAD_PATH"
ASSET="$(asset_for_platform)"
URL="https://github.com/hassard0/Mighty/releases/download/v${VERSION}/${ASSET}"

mkdir -p "$DOWNLOAD_PATH"
curl -fSL "$URL" -o "$DOWNLOAD_PATH/$ASSET"

# Verify sha256 if available.
if curl -fsSL "${URL}.sha256" -o "$DOWNLOAD_PATH/${ASSET}.sha256"; then
  (cd "$DOWNLOAD_PATH" && sha256sum -c "${ASSET}.sha256")
fi
```

### `bin/install`

```bash
#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "$0")/../lib/utils.bash"

VERSION="$ASDF_INSTALL_VERSION"
INSTALL_PATH="$ASDF_INSTALL_PATH"
DOWNLOAD_PATH="$ASDF_DOWNLOAD_PATH"
ASSET="$(asset_for_platform)"

mkdir -p "$INSTALL_PATH/bin"

case "$ASSET" in
  *.tar.gz) tar -xzf "$DOWNLOAD_PATH/$ASSET" -C "$INSTALL_PATH/bin" ;;
  *.zip)    unzip -q "$DOWNLOAD_PATH/$ASSET" -d "$INSTALL_PATH/bin" ;;
  *) echo "unknown asset format: $ASSET" >&2; exit 1 ;;
esac

chmod +x "$INSTALL_PATH/bin/mty" 2>/dev/null \
  || chmod +x "$INSTALL_PATH/bin/mty.exe" 2>/dev/null \
  || true

"$INSTALL_PATH/bin/mty" --version
```

### `lib/utils.bash`

```bash
asset_for_platform() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"

  case "$os-$arch" in
    linux-x86_64)   echo "mty-x86_64-unknown-linux-gnu.tar.gz" ;;
    darwin-arm64|darwin-aarch64) echo "mty-aarch64-apple-darwin.tar.gz" ;;
    *)
      echo "unsupported platform: $os-$arch" >&2
      exit 1
      ;;
  esac
}
```

## Publishing the plugin

```bash
gh repo create hassard0/asdf-mty --public \
  --description "asdf/mise plugin for the Mighty programming language"
# ... create the files above ...
git add . && git commit -m "asdf-mty 0.1.0" && git push -u origin main

# Optionally submit to mise's plugin registry:
#   https://github.com/jdx/mise/blob/main/registry.toml
```

Once the registry PR lands users can drop the URL:

```bash
mise plugin add mty
mise install mty@0.30.1
```
