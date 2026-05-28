# syntax=docker/dockerfile:1.6
#
# Mighty devcontainer base image.
#
# Built on top of the runtime image; adds the supporting toolchains
# most Mighty projects need around their agents:
#   - rust + rust-analyzer  (for native build of mty deps + LSP)
#   - node + npm            (for MCP servers, common host scaffolding)
#   - python3 + pip         (for MCP servers + glue scripts)
#   - git, build-essential  (general dev hygiene)
#
# Build:
#   docker build -t mighty-lang/devcontainer:0.30.1 \
#     tools/distribution/devcontainer
#
# Push:
#   docker push mighty-lang/devcontainer:0.30.1

ARG MTY_VERSION=0.30.1
FROM mighty-lang/mty:${MTY_VERSION} AS mty-stage

FROM debian:bookworm

ARG MTY_VERSION
LABEL org.opencontainers.image.title="Mighty devcontainer"
LABEL org.opencontainers.image.description="VS Code devcontainer base for Mighty projects"
LABEL org.opencontainers.image.source="https://github.com/hassard0/Mighty"
LABEL org.opencontainers.image.licenses="MIT"
LABEL org.opencontainers.image.version="${MTY_VERSION}"

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential \
      ca-certificates \
      curl \
      git \
      gnupg \
      libssl-dev \
      pkg-config \
      python3 \
      python3-pip \
      python3-venv \
      sudo \
      unzip \
      zip \
 && rm -rf /var/lib/apt/lists/*

# Node.js 20 (LTS), via nodesource
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
 && apt-get install -y --no-install-recommends nodejs \
 && rm -rf /var/lib/apt/lists/*

# Create a non-root user for VS Code to attach as
ARG USERNAME=vscode
ARG USER_UID=1000
ARG USER_GID=$USER_UID
RUN groupadd --gid $USER_GID $USERNAME \
 && useradd --uid $USER_UID --gid $USER_GID -m -s /bin/bash $USERNAME \
 && echo "$USERNAME ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/$USERNAME \
 && chmod 0440 /etc/sudoers.d/$USERNAME

USER $USERNAME
WORKDIR /home/$USERNAME

# Rust + rust-analyzer (rustup, stable channel)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain stable --profile minimal \
                  --component rust-analyzer rustfmt clippy
ENV PATH="/home/${USERNAME}/.cargo/bin:${PATH}"

# Drop the mty binary in
USER root
COPY --from=mty-stage /usr/local/bin/mty /usr/local/bin/mty

USER $USERNAME
WORKDIR /workspace

CMD ["bash"]
