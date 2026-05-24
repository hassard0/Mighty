# demos/01_search_api/Dockerfile — multi-stage build that compiles the
# Stardust toolchain from source and bundles the demo into a slim
# runtime image. Intended for the v0.4 alpha-gate "external user can
# install + run an example" check.
#
# Build from the repository root:
#
#   docker build -t stardust-search-api \
#       -f demos/01_search_api/Dockerfile .
#
# Run:
#
#   docker run --rm stardust-search-api

# ---- stage 1: build the compiler --------------------------------------------
FROM rust:1 AS build
WORKDIR /src
COPY . .
RUN cargo build -p sdust-cli --release

# ---- stage 2: minimal runtime ------------------------------------------------
FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /demo
COPY --from=build /src/target/release/sdust /usr/local/bin/sdust
COPY demos/01_search_api/ /demo/

CMD ["sdust", "run", "src/main.sd"]
