# StarryOS

A Linux-compatible monolithic kernel built on [ArceOS](https://github.com/arceos-org/arceos) unikernel, with all dependencies sourced from [crates.io](https://crates.io). Supports multiple architectures via `cargo xtask`.

## Supported Architectures

| Architecture | Rust Target | QEMU Machine | Platform |
|---|---|---|---|
| riscv64 | `riscv64gc-unknown-none-elf` | `qemu-system-riscv64 -machine virt` | riscv64-qemu-virt |
| aarch64 | `aarch64-unknown-none-softfloat` | `qemu-system-aarch64 -machine virt` | aarch64-qemu-virt |
| x86_64 | `x86_64-unknown-none` | `qemu-system-x86_64 -machine q35` | x86-pc |
| loongarch64 | `loongarch64-unknown-none-softfloat` | `qemu-system-loongarch64 -machine virt` | loongarch64-qemu-virt |

## Prerequisites

- **Rust nightly toolchain** (edition 2024)

  ```bash
  rustup install nightly
  rustup default nightly
  ```

- **Bare-metal targets** (install the ones you need)

  ```bash
  rustup target add riscv64gc-unknown-none-elf
  rustup target add aarch64-unknown-none-softfloat
  rustup target add x86_64-unknown-none
  rustup target add loongarch64-unknown-none
  ```

- **QEMU** (install the emulators for your target architectures)

  ```bash
  # Ubuntu/Debian
  sudo apt install qemu-system-riscv64 qemu-system-aarch64 \
                   qemu-system-x86 qemu-system-loongarch64  # OR qemu-system-misc

  # macOS (Homebrew)
  brew install qemu
  ```

  > **Note:** Running on LoongArch64 requires QEMU 10+. If the version in your distribution is too old, consider building QEMU from [source](https://www.qemu.org/download/).

- **Musl cross-compilation toolchain** (needed for building lwext4 C library)

  1. Download from [setup-musl releases](https://github.com/arceos-org/setup-musl/releases/tag/prebuilt)
  2. Extract and add to `PATH`:

  ```bash
  export PATH=/opt/riscv64-linux-musl-cross/bin:$PATH
  ```

- **rust-objcopy** (from `cargo-binutils`)

  ```bash
  cargo install cargo-binutils
  rustup component add llvm-tools
  ```

## Quick Start

```bash
# Install cargo-clone sub-command
cargo install cargo-clone

# Get source code of starryos crate from crates.io
cargo clone starryos
cd starryos

# Download root filesystem image (default: riscv64)
cargo xtask rootfs

# Build and run on RISC-V 64 QEMU (default)
cargo xtask run

# Build and run on other architectures
cargo xtask run --arch aarch64
cargo xtask run --arch x86_64
cargo xtask run --arch loongarch64

# Build only (no QEMU)
cargo xtask build --arch riscv64
```

### xtask Options

```
cargo xtask run [OPTIONS]

Options:
    --arch <ARCH>          Target architecture [default: riscv64; other: aarch64, x86_64, loongarch64]
    --log <LOG>            Logging level: off, error, warn, info, debug, trace [default: warn]
    --mode <MODE>          Build mode: release, debug [default: release]
    --mem <MEM>            Memory size (e.g., 1G, 512M) [default: 1G]
    --smp <SMP>            Number of CPUs (default: from platform config)
    --no-dwarf             Disable DWARF debug info
    --no-blk               Disable block device (virtio-blk)
    --no-net               Disable network device (virtio-net)
    --bus <BUS>            Device bus type: pci, mmio [default: pci]
    --disk-img <PATH>      Disk image file [default: rootfs-<arch>.img]
```

### Expected Output (riscv64)

```
       d8888                            .d88888b.   .d8888b.
      d88888                           d88P" "Y88b d88P  Y88b
     ...
d88P     888 888      "Y8888P  "Y8888   "Y88888P"   "Y8888P"

arch = riscv64
platform = riscv64-qemu-virt
...
smp = 1

Welcome to Starry OS!
starry:~#
```

StarryOS boots into an interactive shell. Press `Ctrl-A X` to exit QEMU.

## Project Structure

```
starryos/
├── .cargo/
│   └── config.toml       # cargo xtask alias
├── xtask/
│   └── src/
│       └── main.rs       # Build/run/rootfs subcommand implementation
├── configs/
│   └── defconfig.toml    # Default kernel configuration
├── scripts/
│   └── make/
│       └── dwarf.sh      # DWARF debug section handling
├── src/
│   ├── main.rs           # Kernel entry point
│   ├── entry.rs          # Init process setup
│   └── init.sh           # Init shell script
├── build.rs              # Linker script setup (auto-detects arch)
├── Cargo.toml            # Dependencies (all from crates.io)
└── README.md
```

## How It Works

The `cargo xtask` pattern uses a host-native helper binary (`xtask/`) to orchestrate
cross-compilation and QEMU execution:

1. **`cargo xtask build --arch <ARCH>`**
   - Installs required tools (`cargo-axplat`, `axconfig-gen`, `cargo-binutils`)
   - Resolves platform configuration via `cargo axplat info`
   - Generates `.axconfig.toml` with architecture and memory settings
   - Runs `cargo build --release --target <TARGET> --features "qemu ..."`
   - Processes DWARF debug info and converts ELF to raw binary

2. **`cargo xtask run --arch <ARCH>`**
   - Performs the build step above
   - Launches QEMU with virtio-blk (rootfs), virtio-net, and architecture-specific flags

3. **`cargo xtask rootfs --arch <ARCH>`**
   - Downloads the root filesystem image from [Starry-OS/rootfs](https://github.com/Starry-OS/rootfs/releases)

## Key Components

| Component | Role |
|---|---|
| `axfeat` | ArceOS feature aggregator (selects kernel modules) |
| `axhal` | Hardware abstraction layer, generates the linker script at build time |
| `axplat-*` | Platform-specific support crates (one per target board/VM) |
| `axruntime` | Kernel initialization and runtime setup |
| `starry-api` | Linux-compatible syscall API layer |
| `starry-core` | Core OS components: process, memory, filesystem management |
| `starry-process` | Process and thread management |
| `starry-signal` | POSIX signal handling |
| `build.rs` | Locates the linker script generated by `axhal` and passes it to the linker |
| `configs/defconfig.toml` | Default kernel configuration (task stack size, timer frequency) |

## License

This project is released under the Apache License 2.0. See the [LICENSE](./LICENSE) and [NOTICE](./NOTICE) files for details.
