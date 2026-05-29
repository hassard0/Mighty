# syntax=docker/dockerfile:1.6
#
# Mighty official runtime image.
#
# Two-stage build:
#   stage 1 (builder): fetches and verifies the released Linux tarball
#                      for the active TARGETARCH (amd64 or arm64)
#   stage 2 (runtime): minimal debian-slim image with just the mty binary
#
# Resulting image is ~50MB and ships only the mty binary + minimal
# glibc + ca-certificates (needed for `mty install` to fetch deps).
#
# Build (single-arch, host-native):
#   docker build --build-arg MTY_VERSION=0.35.0 \
#     -t mighty-lang/mty:0.35.0 -t mighty-lang/mty:latest \
#     tools/distribution/docker
#
# Build (multi-arch, via buildx):
#   docker buildx build --platform linux/amd64,linux/arm64 \
#     --build-arg MTY_VERSION=0.35.0 \
#     -t ghcr.io/hassard0/mty:0.35.0 \
#     -t ghcr.io/hassard0/mty:latest \
#     --push tools/distribution/docker
#
# Run:
#   docker run --rm -v "$PWD:/work" -w /work mighty-lang/mty:0.35.0 \
#     check src/main.mty
#
# v0.35 (Track T4): the Dockerfile no longer hard-codes the
# x86_64-unknown-linux-gnu asset URL. The `TARGETARCH` build-arg
# (auto-populated by buildx — `amd64` or `arm64`) selects the
# matching Rust target triple in the asset name. The SHA256 is
# fetched from the `.sha256` sidecar published alongside the
# release tarball rather than baked into this Dockerfile, so a
# point release doesn't need a Dockerfile edit.

ARG MTY_VERSION=0.35.0

# -----------------------------------------------------------------------------
# Stage 1: fetch + verify
# -----------------------------------------------------------------------------
FROM debian:bookworm-slim AS builder

# TARGETARCH is supplied by `docker buildx build --platform=...`.
# For a plain `docker build` on a host without buildx it defaults
# to the host's GOARCH-style label (amd64/arm64).
ARG TARGETARCH
ARG MTY_VERSION

RUN apt-get update \
 && apt-get install -y --no-install-recommends curl ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /work

# v0.35 (Track T4): TARGETARCH-aware asset URL. The release.yml
# matrix produces `mty-x86_64-unknown-linux-gnu.tar.gz` and
# `mty-aarch64-unknown-linux-gnu.tar.gz` tarballs plus matching
# `.sha256` sidecars; this step picks the right pair, downloads
# both, and verifies the tarball against the sidecar's checksum.
RUN set -eux; \
    case "$TARGETARCH" in \
      amd64) ARCH_TRIPLE="x86_64-unknown-linux-gnu" ;; \
      arm64) ARCH_TRIPLE="aarch64-unknown-linux-gnu" ;; \
      *)     echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    ASSET="mty-${ARCH_TRIPLE}.tar.gz"; \
    BASE="https://github.com/hassard0/Mighty/releases/download/v${MTY_VERSION}"; \
    curl -fsSL "${BASE}/${ASSET}"        -o /tmp/mty.tar.gz; \
    curl -fsSL "${BASE}/${ASSET}.sha256" -o /tmp/mty.tar.gz.sha256.raw; \
    HASH="$(awk '{ print $1 }' /tmp/mty.tar.gz.sha256.raw)"; \
    printf '%s  /tmp/mty.tar.gz\n' "$HASH" > /tmp/mty.tar.gz.sha256; \
    sha256sum -c /tmp/mty.tar.gz.sha256; \
    tar -xzf /tmp/mty.tar.gz -C /tmp; \
    install -m 0755 "/tmp/mty-v${MTY_VERSION}/mty" /work/mty; \
    /work/mty --version

# Notes on the RUN above:
#   - The published `.sha256` sidecar embeds the original filename
#     (e.g. "<hash>  mty-x86_64-unknown-linux-gnu.tar.gz"). We
#     rebuild the check line in the standard `<hash>  <file>`
#     (two-space) form against our local /tmp path so
#     `sha256sum -c` finds the file. Hash column is preserved
#     untouched, so we still pin to the release-side checksum.
#   - The tarball expands to `mty-v<version>/mty` (versioned dir
#     name set in release.yml's Package step), so we have to lift
#     the binary up one level before stage 2 copies it across.

# -----------------------------------------------------------------------------
# Stage 2: runtime
# -----------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime
ARG MTY_VERSION

LABEL org.opencontainers.image.title="Mighty (mty)"
LABEL org.opencontainers.image.description="Mighty - agent-first systems programming language"
LABEL org.opencontainers.image.source="https://github.com/hassard0/Mighty"
LABEL org.opencontainers.image.url="https://hassard0.github.io/Mighty/"
LABEL org.opencontainers.image.documentation="https://hassard0.github.io/Mighty/"
LABEL org.opencontainers.image.licenses="MIT"
LABEL org.opencontainers.image.vendor="Mighty"
LABEL org.opencontainers.image.version="${MTY_VERSION}"

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --create-home --shell /bin/bash mighty

COPY --from=builder /work/mty /usr/local/bin/mty

USER mighty
WORKDIR /work

ENTRYPOINT ["mty"]
CMD ["--help"]
