# StarryOS Dockerfile
# Provides a consistent Linux build environment for StarryOS
# This solves the issue where macOS builds fail due to hardcoded compiler names in lwext4_rust

# Use a lighter base image since we install a specific Rust version anyway
FROM debian:bookworm-slim

# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    cmake \
    clang \
    qemu-system \
    curl \
    xz-utils \
    git \
    make \
    python3 \
    dos2unix \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Install Musl toolchains for all supported architectures
# Download prebuilt toolchains from arceos-org/setup-musl (prebuilt release)
RUN mkdir -p /opt/toolchains && \
    cd /opt/toolchains && \
    for arch in x86_64 aarch64 riscv64 loongarch64; do \
    curl -f -L "https://github.com/arceos-org/setup-musl/releases/download/prebuilt/${arch}-linux-musl-cross.tgz" -o "${arch}-linux-musl-cross.tgz" && \
    tar -xzf "${arch}-linux-musl-cross.tgz" && \
    rm "${arch}-linux-musl-cross.tgz"; \
    done

# Make Musl toolchains available in PATH
ENV PATH="/opt/toolchains/x86_64-linux-musl-cross/bin:/opt/toolchains/aarch64-linux-musl-cross/bin:/opt/toolchains/riscv64-linux-musl-cross/bin:/opt/toolchains/loongarch64-linux-musl-cross/bin:$PATH"

# Install Rust using rustup
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
ENV PATH="/root/.cargo/bin:${PATH}"
# Set up Rust toolchain
# Copy rust-toolchain.toml first - rustup will automatically use it
WORKDIR /workspace

# Copy rust-toolchain.toml - rustup will detect and use it
COPY rust-toolchain.toml /workspace/rust-toolchain.toml

# Install the Rust toolchain specified in rust-toolchain.toml
# Simply entering the directory and invoking Cargo will let rustup
# read rust-toolchain.toml and install the correct toolchain,
# components, and targets automatically.
RUN cd /workspace && \
    cargo -V

# Install cargo tools
RUN cargo install cargo-binutils axconfig-gen cargo-axplat

# Set working directory
WORKDIR /workspace

# Default command
CMD ["/bin/bash"]

