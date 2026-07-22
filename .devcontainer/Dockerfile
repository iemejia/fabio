FROM ubuntu:26.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    build-essential \
    cmake \
    pkg-config \
    libssl-dev \
    musl-tools \
    sudo \
    lld \
    clang \
    git \
    && rm -rf /var/lib/apt/lists/*

# Install zig (needed by cargo-zigbuild for macOS/arm64 cross-compilation)
ARG ZIG_VERSION=0.16.0
RUN curl -sSL "https://ziglang.org/download/${ZIG_VERSION}/zig-$(uname -m)-linux-${ZIG_VERSION}.tar.xz" \
    | tar -xJ -C /opt \
    && ln -s /opt/zig-$(uname -m)-linux-${ZIG_VERSION}/zig /usr/local/bin/zig

# Reuse existing ubuntu user (UID 1000) and grant passwordless sudo
RUN echo "ubuntu ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers.d/ubuntu \
    && chmod 0440 /etc/sudoers.d/ubuntu

# Install Rust toolchain as ubuntu user (matches remoteUser in devcontainer.json)
USER ubuntu
ENV PATH="/home/ubuntu/.cargo/bin:${PATH}"

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    --default-toolchain stable \
    --component rustfmt,clippy \
    --target x86_64-unknown-linux-musl,aarch64-unknown-linux-musl,x86_64-apple-darwin,aarch64-apple-darwin,x86_64-pc-windows-msvc

# Install cargo dev tools (baked in for fast container startup)
RUN cargo install git-cliff cargo-zigbuild cargo-xwin cargo-audit prek \
    && rm -rf /home/ubuntu/.cargo/registry /home/ubuntu/.cargo/git
