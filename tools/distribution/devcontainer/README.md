# Mighty devcontainer

Drop-in VS Code devcontainer for Mighty projects. Pre-installs the
`mty` toolchain plus Rust, Node 20, and Python 3 so you can write
the host scaffolding around your Mighty agents in the same workspace.

## End-user setup

1. Copy `devcontainer.json` into your project at
   `.devcontainer/devcontainer.json`.
2. Open the project in VS Code with the "Dev Containers" extension
   installed.
3. Run **Dev Containers: Reopen in Container**.

The first build pulls `mighty-lang/devcontainer:0.30.1` (~700MB) and
caches it locally. `mty --version` prints on container start.

## Building the base image

The devcontainer pulls a published image; you only need to rebuild it
when shipping a new Mighty release or changing the included
toolchains.

```bash
# From the Mighty source tree, requires the runtime image first:
docker build -t mighty-lang/mty:0.30.1 tools/distribution/docker

docker build \
  -t mighty-lang/devcontainer:0.30.1 \
  -t mighty-lang/devcontainer:latest \
  tools/distribution/devcontainer
```

## Publishing

```bash
docker push mighty-lang/devcontainer:0.30.1
docker push mighty-lang/devcontainer:latest

# Or to GHCR:
docker tag mighty-lang/devcontainer:0.30.1 ghcr.io/hassard0/mty-devcontainer:0.30.1
docker push ghcr.io/hassard0/mty-devcontainer:0.30.1
```

## VS Code extension reference

`devcontainer.json` references `hassard0.mighty-language`, the
extension produced by v0.31 Track 2 (LSP + syntax highlighting). Until
the extension is published to the marketplace, users will see a
"could not install extension" message on container start. Either:

- Sideload the `.vsix` via **Extensions: Install from VSIX** before
  reopening in the container, or
- Remove that entry from the `extensions` array in their copy of
  `devcontainer.json`.

This warning goes away once Track 2 publishes the extension.

## Per-release checklist

1. Update `ARG MTY_VERSION=` default in `Dockerfile`.
2. Update the `"image":` tag in `devcontainer.json`.
3. Rebuild and push both `:X.Y.Z` and `:latest` tags.
