# Starry OS

- [简体中文版](./README-zh_CN.md)
- [English Version](./README.md)

## 快速入手

### 1. 安装系统依赖

此步骤可能因你的操作系统而异。以下是基于 Debian 的示例：

```bash
$ sudo apt update
$ sudo apt install -y build-essential cmake clang qemu-system
```

**注意：** 在 LoongArch64 上运行需要 QEMU 10。如果你的 Linux 发行版中的 QEMU 版本太旧（例如 Ubuntu 中），可以考虑从[源代码](https://www.qemu.org/download/)安装 QEMU。

### 2. 安装 Musl 工具链

1. 从 [预构建的 musl](https://github.com/arceos-org/setup-musl/releases/tag/prebuilt) 下载文件.
2. 解压到某个路径，例如 `/opt/riscv64-linux-musl-cross`。
3. 将 bin 文件夹添加到 `PATH`，例如：

   ```bash
   $ export PATH=/opt/riscv64-linux-musl-cross/bin:$PATH
   ```

### 3. 克隆仓库

```bash
$ git clone --recursive https://github.com/Starry-OS/StarryOS.git
$ cd StarryOS
```

或者如果你已经在未使用 `--recursive` 选项时克隆了：

```bash
$ cd StarryOS
$ git submodule update --init --recursive
```

### 4. 设置 Rust 工具链

```bash
# 从 https://rustup.rs 安装 或者使用系统包管理器安装 rustup

# 确保没有设置 `RUSTUP_DIST_SERVER`变量
$ export RUSTUP_DIST_SERVER=

# 通过 rustup 自动下载组件
$ cd StarryOS
$ rustup target list --installed
```

### 5. 构建

```bash
# 默认目标架构t: riscv64
$ make build
# 指定目标架构
$ make ARCH=riscv64 build
$ make ARCH=loongarch64 build
```

此时应同时下载所需的二进制依赖项，如 [cargo-binutils](https://github.com/rust-embedded/cargo-binutils)。

### 6. 准备根文件系统

```bash
$ make img
$ make img ARCH=riscv64
$ make img ARCH=loongarch64
```

这会从 [GitHub Releases](https://github.com/Starry-OS/StarryOS/releases) 下载 rootfs 镜像，并设置在 QEMU 上运行的磁盘文件。

### 7. Run on QEMU

```bash
$ make run ARCH=riscv64
$ make run ARCH=loongarch64

# 快捷命令:
$ make rv
$ make la
```

注：

1. 在每次运行前不必重新构建。`run` 会自动重新构建。
2. 每次运行之间磁盘文件**不会**被重置。因此，如果你想切换到另一种架构，则必须在运行 `make run` 之前使用新架构运行 `make img`。

## 下一步呢?

你可以查看 [GUI 指南](./docs/gui.md)来设置图形界面，或者浏览文件夹中的其他文档。

## 其他选项

待办

查看 [Makefile](./Makefile)

## 许可

本项目在 Apache 许可证 2.0 下发布。项目中的所有修改和新增贡献均在相同的许可证下分发。详细信息请参见 [LICENSE](./LICENSE) 和 [NOTICE](./NOTICE) 文件。
