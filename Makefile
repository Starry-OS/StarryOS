# StarryOS Standalone Build System
# (All ArceOS components from crates.io, no local arceos submodule needed)
#
# Available arguments:
# * General options:
#     - `ARCH`: Target architecture: x86_64, riscv64, aarch64, loongarch64
#     - `MYPLAT`: Package name of the target platform crate.
#     - `PLAT_CONFIG`: Path to the platform configuration file.
#     - `SMP`: Number of CPUs. If not set, use the default value from platform config.
#     - `MODE`: Build mode: release, debug
#     - `LOG:` Logging level: warn, error, info, debug, trace
#     - `V`: Verbose level: (empty), 1, 2
#     - `DWARF`: Enable DWARF debug info: y, n
# * QEMU options:
#     - `BLK`: Enable storage devices (virtio-blk)
#     - `NET`: Enable network devices (virtio-net)
#     - `MEM`: Memory size (default is 1G)
#     - `BUS`: Device bus type: mmio, pci

# Build Options
ARCH ?= riscv64
LOG ?= warn
DWARF ?= y
export DWARF
MEMTRACK ?= n

# QEMU Options
BLK ?= y
NET ?= y
VSOCK ?= n
MEM ?= 1G
ICOUNT ?= n

# App Options
A ?= $(PWD)
APP ?= $(A)
NO_AXSTD ?= y
AX_LIB ?= axfeat
APP_FEATURES ?= qemu

ifeq ($(MEMTRACK), y)
	APP_FEATURES += starry-api/memtrack
endif

# General options
MYPLAT ?=
PLAT_CONFIG ?=
SMP ?=
MODE ?= release
V ?=
LTO ?=
TARGET_DIR ?= $(PWD)/target
EXTRA_CONFIG ?=
OUT_CONFIG ?= $(PWD)/.axconfig.toml
UIMAGE ?= n
FEATURES ?=

# QEMU options
GRAPHIC ?= n
INPUT ?= n
BUS ?= pci
ACCEL ?=
QEMU_ARGS ?=
DISK_IMG ?= rootfs-$(ARCH).img
QEMU_LOG ?= n
NET_DUMP ?= n
NET_DEV ?= user
VFIO_PCI ?=
VHOST ?= n

# Network options
IP ?= 10.0.2.15
GW ?= 10.0.2.2

# App type
ifeq ($(wildcard $(APP)),)
  $(error Application path "$(APP)" is not valid)
endif

ifneq ($(wildcard $(APP)/Cargo.toml),)
  APP_TYPE := rust
else
  APP_TYPE := c
endif

.DEFAULT_GOAL := all

ifneq ($(filter $(or $(MAKECMDGOALS), $(.DEFAULT_GOAL)), all build disasm run justrun debug defconfig oldconfig),)
# Install dependencies
include scripts/make/deps.mk
# Platform resolving
include scripts/make/platform.mk
# Configuration generation
include scripts/make/config.mk
# Feature parsing
include scripts/make/features.mk
endif

# Target
ifeq ($(ARCH), x86_64)
  TARGET := x86_64-unknown-none
else ifeq ($(ARCH), aarch64)
  TARGET := aarch64-unknown-none-softfloat
else ifeq ($(ARCH), riscv64)
  TARGET := riscv64gc-unknown-none-elf
else ifeq ($(ARCH), loongarch64)
  TARGET := loongarch64-unknown-none-softfloat
else
  $(error "ARCH" must be one of "x86_64", "riscv64", "aarch64" or "loongarch64")
endif

export AX_ARCH=$(ARCH)
export AX_PLATFORM=$(PLAT_NAME)
export AX_MODE=$(MODE)
export AX_LOG=$(LOG)
export AX_TARGET=$(TARGET)
export AX_IP=$(IP)
export AX_GW=$(GW)
export AX_CONFIG_PATH=$(OUT_CONFIG)

# Binutils
CROSS_COMPILE ?= $(ARCH)-linux-musl-
CC := $(CROSS_COMPILE)gcc
AR := $(CROSS_COMPILE)ar
RANLIB := $(CROSS_COMPILE)ranlib
LD := rust-lld -flavor gnu

OBJDUMP ?= rust-objdump -d --print-imm-hex --x86-asm-syntax=intel
OBJCOPY ?= rust-objcopy --binary-architecture=$(ARCH)
GDB ?= gdb

# Paths
OUT_DIR ?= $(APP)
LD_SCRIPT ?= $(TARGET_DIR)/$(TARGET)/$(MODE)/linker_$(PLAT_NAME).lds

APP_NAME := $(shell basename $(APP))
OUT_ELF := $(OUT_DIR)/$(APP_NAME)_$(PLAT_NAME).elf
OUT_BIN := $(patsubst %.elf,%.bin,$(OUT_ELF))
OUT_UIMG := $(patsubst %.elf,%.uimg,$(OUT_ELF))
ifeq ($(UIMAGE), y)
  FINAL_IMG := $(OUT_UIMG)
else
  FINAL_IMG := $(OUT_BIN)
endif

all: build

include scripts/make/utils.mk
include scripts/make/build.mk
include scripts/make/qemu.mk

# Rootfs
ROOTFS_URL = https://github.com/Starry-OS/rootfs/releases/download/20250917
ROOTFS_IMG = rootfs-$(ARCH).img

rootfs:
	@if [ ! -f $(ROOTFS_IMG) ]; then \
		echo "Image not found, downloading..."; \
		curl -f -L $(ROOTFS_URL)/$(ROOTFS_IMG).xz -O; \
		xz -d $(ROOTFS_IMG).xz; \
	fi

img:
	@echo -e "\033[33mWARN: The 'img' target is deprecated. Please use 'rootfs' instead.\033[0m"
	@$(MAKE) --no-print-directory rootfs

defconfig:
	$(call defconfig)

oldconfig:
	$(call oldconfig)

build: defconfig $(OUT_DIR) $(FINAL_IMG)

disasm:
	$(OBJDUMP) $(OUT_ELF) | less

run: build justrun

justrun:
	$(call run_qemu)

debug: build
	$(call run_qemu_debug) &
	$(GDB) $(OUT_ELF) \
	  -ex 'target remote localhost:1234' \
	  -ex 'b __axplat_main' \
	  -ex 'continue' \
	  -ex 'disp /16i $$pc'

# Aliases
rv:
	$(MAKE) ARCH=riscv64 run

la:
	$(MAKE) ARCH=loongarch64 run

vf2:
	$(MAKE) ARCH=riscv64 APP_FEATURES=vf2 MYPLAT=axplat-riscv64-visionfive2 BUS=mmio build

clean:
	rm -rf $(APP)/*.bin $(APP)/*.elf $(OUT_CONFIG)
	cargo clean

.PHONY: all defconfig oldconfig build disasm run justrun debug \
	rootfs img clean rv la vf2
