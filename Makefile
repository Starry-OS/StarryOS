# Build Options
export ARCH := $(ARCH)
export LOG := warn
export DWARF := y
export MEMTRACK := n
export BUILTIN_KEBPF := y
export TARGET_DIR := $(PWD)/target

# QEMU Options
export BLK := y
export NET := y
export VSOCK := n
export MEM := 1G
export ICOUNT := n

# Generated Options
export A := $(PWD)/starryos
export NO_AXSTD := y
export AX_LIB := axfeat
export APP_FEATURES := qemu
export LOCAL_MNT := $(PWD)/mnt
export KMOD_LINKER_SCRIPT := $(PWD)/modules/kmod-linker.ld

ifeq ($(ARCH), )
    export ARCH := riscv64
endif

ifeq ($(MEMTRACK), y)
	APP_FEATURES += starry-api/memtrack
endif

ifeq ($(filter modules, $(MAKECMDGOALS)),)
	ifeq ($(BUILTIN_KEBPF), y)
		APP_FEATURES += kebpf
	endif
endif

include modules/kmod.mk

default: build

ROOTFS_URL = https://github.com/Starry-OS/rootfs/releases/download/20260214
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

mount umount: rootfs
	@$(MAKE) -C make $@

defconfig justrun clean:
	@$(MAKE) -C make $@

build run debug disasm: defconfig
	@$(MAKE) -C make $@

build_apps:
	@$(MAKE) -C user/musl build

copy_apps: mount
	@-$(MAKE) -C user/musl write
	@$(MAKE) -C make umount

apps: build_apps copy_apps

build_modules: $(MODULES)

$(MODULES):
	@echo "Building kernel module: $@"
	@KMOD=y APP=$@ $(MAKE) -C make build

copy_modules: mount
	@-$(foreach module, $(MODULES), (KMOD=y APP=$(module) $(MAKE) -C make copy_ko;);)
	@$(MAKE) -C make umount

modules: build_modules copy_modules

ci-test:
	./scripts/ci-test.py $(ARCH)

# Aliases
rv:
	$(MAKE) ARCH=riscv64 run

la:
	$(MAKE) ARCH=loongarch64 run

vf2:
	$(MAKE) ARCH=riscv64 APP_FEATURES=vf2 MYPLAT=axplat-riscv64-visionfive2 BUS=mmio build

.PHONY: build run justrun debug disasm clean build_apps copy_apps build_modules $(MODULES)
