# shellcheck shell=bash
#
# Shared helpers for asdf-mty plugin scripts.
#
# Sourced by `bin/list-all`, `bin/download`, and `bin/install`.

set -euo pipefail

GITHUB_REPO="${GITHUB_REPO_OVERRIDE:-hassard0/Mighty}"
GITHUB_HOST="${GITHUB_HOST_OVERRIDE:-https://github.com}"
GITHUB_API="${GITHUB_API_OVERRIDE:-https://api.github.com}"

fail() {
  echo "asdf-mty: $*" >&2
  exit 1
}

# Map `uname -s` / `uname -m` to a Mighty release asset filename.
# Must stay in sync with the `release.yml` build matrix.
asset_for_platform() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"

  case "$os-$arch" in
    linux-x86_64|linux-amd64)
      echo "mty-x86_64-unknown-linux-gnu.tar.gz"
      ;;
    linux-aarch64|linux-arm64)
      echo "mty-aarch64-unknown-linux-gnu.tar.gz"
      ;;
    darwin-arm64|darwin-aarch64)
      echo "mty-aarch64-apple-darwin.tar.gz"
      ;;
    darwin-x86_64|darwin-amd64)
      echo "mty-x86_64-apple-darwin.tar.gz"
      ;;
    msys*|mingw*|cygwin*)
      # Windows under git-bash / msys2. asdf-on-Windows is rare,
      # but the asset is published, so we honor it.
      echo "mty-x86_64-pc-windows-msvc.zip"
      ;;
    *)
      fail "unsupported platform: $os-$arch (open an issue at $GITHUB_HOST/$GITHUB_REPO/issues)"
      ;;
  esac
}

# Build the download URL for a given version + asset filename.
release_asset_url() {
  local version="$1"
  local asset="$2"
  echo "${GITHUB_HOST}/${GITHUB_REPO}/releases/download/v${version}/${asset}"
}

# Verify the downloaded asset against its .sha256 sidecar. Mighty's
# release.yml publishes sidecars in the format `<hash>  <filename>`
# (two-space separator, GNU coreutils default). `sha256sum -c` is
# the canonical verifier on Linux; macOS ships `shasum` instead, so
# we feature-detect.
verify_sha256() {
  local dir="$1"
  local asset="$2"
  local sha_file="$3"

  (
    cd "$dir"
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum -c "$(basename "$sha_file")"
    elif command -v shasum >/dev/null 2>&1; then
      shasum -a 256 -c "$(basename "$sha_file")"
    else
      echo "warning: no sha256sum / shasum available — skipping integrity check" >&2
    fi
  )
}
