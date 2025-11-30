# StarryOS Dockerfile
# Provides a consistent Linux build environment for StarryOS
# This solves the issue where macOS builds fail due to hardcoded compiler names in lwext4_rust

FROM rust:1.80-slim

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
    dos2unix \
    && rm -rf /var/lib/apt/lists/*

# Install Musl toolchain for RISC-V
# Using musl.cc which provides pre-built cross-compilers
RUN mkdir -p /opt/toolchains && \
    cd /opt/toolchains && \
    curl -f -L https://musl.cc/riscv64-linux-musl-cross.tgz -o riscv64-linux-musl-cross.tgz && \
    tar -xzf riscv64-linux-musl-cross.tgz && \
    rm riscv64-linux-musl-cross.tgz

# Install Musl toolchain for LoongArch64 (optional, for LoongArch support)
RUN cd /opt/toolchains && \
    curl -f -L https://github.com/LoongsonLab/oscomp-toolchains-for-oskernel/releases/download/loongarch64-linux-musl-cross-gcc-13.2.0/loongarch64-linux-musl-cross.tgz -o loongarch64-linux-musl-cross.tgz && \
    tar -xzf loongarch64-linux-musl-cross.tgz && \
    rm loongarch64-linux-musl-cross.tgz || echo "LoongArch toolchain download failed, continuing without it"

# Set up Rust toolchain
# Copy rust-toolchain.toml first - rustup will automatically use it
WORKDIR /workspace

# Copy rust-toolchain.toml - rustup will detect and use it
COPY rust-toolchain.toml /workspace/rust-toolchain.toml

# Install the toolchain specified in rust-toolchain.toml
# rustup will automatically read the file when we change to the directory
RUN cd /workspace && \
    rustup toolchain install nightly-2025-05-20 && \
    rustup default nightly-2025-05-20 && \
    rustup component add rust-src llvm-tools rustfmt clippy && \
    rustup target add x86_64-unknown-none riscv64gc-unknown-none-elf aarch64-unknown-none-softfloat loongarch64-unknown-none-softfloat

# Install cargo tools
RUN cargo install cargo-binutils

# Set environment variables
ENV PATH="/opt/toolchains/riscv64-linux-musl-cross/bin:/opt/toolchains/loongarch64-linux-musl-cross/bin:$PATH"
ENV RUSTUP_DIST_SERVER=""

# Create entrypoint script to fix line endings
RUN printf '#!/bin/bash\n\
# Fix line endings for shell scripts\n\
find /workspace -type f -name "*.sh" -exec dos2unix {} + 2>/dev/null || true\n\
# Execute the command passed to the container\n\
exec "$@"\n' > /entrypoint.sh && \
    chmod +x /entrypoint.sh

# Set working directory
WORKDIR /workspace

# Use entrypoint to fix line endings before running commands
ENTRYPOINT ["/entrypoint.sh"]
CMD ["/bin/bash"]

