# syntax=docker/dockerfile:1.6
#
# Mighty official runtime image.
#
# Two-stage build:
#   stage 1 (builder): fetches and verifies the released Linux tarball
#   stage 2 (runtime): minimal debian-slim image with just the mty binary
#
# Resulting image is ~50MB and ships only the mty binary + minimal
# glibc + ca-certificates (needed for `mty install` to fetch deps).
#
# Build:
#   docker build -t mighty-lang/mty:0.30.1 -t mighty-lang/mty:latest \
#     tools/distribution/docker
#
# Run:
#   docker run --rm -v "$PWD:/work" -w /work mighty-lang/mty:0.30.1 \
#     check src/main.mty
#
# Override version at build time:
#   docker build --build-arg MTY_VERSION=0.31.0 -t mighty-lang/mty:0.31.0 .

ARG MTY_VERSION=0.30.1
ARG MTY_SHA256=c5bb431ea6d3e57c0952ecde6d9943281d00d513d388ae6f55722e810031c602

# -----------------------------------------------------------------------------
# Stage 1: fetch + verify
# -----------------------------------------------------------------------------
FROM debian:bookworm-slim AS builder
ARG MTY_VERSION
ARG MTY_SHA256

RUN apt-get update \
 && apt-get install -y --no-install-recommends curl ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /work

RUN curl -fsSL \
      "https://github.com/hassard0/Mighty/releases/download/v${MTY_VERSION}/mty-x86_64-unknown-linux-gnu.tar.gz" \
      -o mty.tar.gz \
 && echo "${MTY_SHA256}  mty.tar.gz" | sha256sum -c - \
 && tar -xzf mty.tar.gz \
 && chmod +x mty \
 && ./mty --version

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
