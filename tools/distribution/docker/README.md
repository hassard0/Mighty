# Mighty Docker image

Official runtime image for the `mty` toolchain. Multi-stage build,
debian-slim base, ~50MB final image.

## Build

```bash
# From the Mighty source tree:
docker build \
  -t mighty-lang/mty:0.30.1 \
  -t mighty-lang/mty:latest \
  tools/distribution/docker
```

To rebuild against a different release pin without editing the file:

```bash
docker build \
  --build-arg MTY_VERSION=0.31.0 \
  --build-arg MTY_SHA256=__FILL_FROM_RELEASE_PAGE__ \
  -t mighty-lang/mty:0.31.0 \
  tools/distribution/docker
```

The build will refuse to proceed if the SHA256 doesn't match the
downloaded tarball.

## Run

```bash
# Show help
docker run --rm mighty-lang/mty:0.30.1

# Check a file in $PWD
docker run --rm -v "$PWD:/work" -w /work mighty-lang/mty:0.30.1 \
  check src/main.mty

# Interactive shell
docker run --rm -it --entrypoint bash -v "$PWD:/work" -w /work \
  mighty-lang/mty:0.30.1
```

A `docker-compose.example.yml` is included for project-level use.

## Publish to Docker Hub

```bash
# Authenticate (interactive token):
docker login

# Push both tags:
docker push mighty-lang/mty:0.30.1
docker push mighty-lang/mty:latest
```

For GitHub Container Registry (`ghcr.io`):

```bash
echo "$GHCR_TOKEN" | docker login ghcr.io -u hassard0 --password-stdin

docker tag mighty-lang/mty:0.30.1 ghcr.io/hassard0/mty:0.30.1
docker tag mighty-lang/mty:0.30.1 ghcr.io/hassard0/mty:latest
docker push ghcr.io/hassard0/mty:0.30.1
docker push ghcr.io/hassard0/mty:latest
```

## Per-release checklist

1. Bump the `ARG MTY_VERSION=` and `ARG MTY_SHA256=` defaults in
   `Dockerfile` (fetch the SHA from the release page).
2. Rebuild with the new version + `latest` tags.
3. Push both tags to whichever registry you publish to.
4. (Optional) Sign the image with `cosign` and add a SBOM via
   `docker buildx build --sbom=true --provenance=true`.

## Future work

- Multi-arch build (`linux/amd64`, `linux/arm64`) once an aarch64
  Linux binary ships in release.yml.
- Distroless variant for ultra-minimal images once `mty` is verified
  to run without any glibc/system tooling.
