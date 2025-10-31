# Build Options
export ARCH := riscv64
export LOG := warn
export BACKTRACE := y
export MEMTRACK := n

# QEMU Options
export BLK := y
export NET := y
export VSOCK := n
export MEM := 1G
export ICOUNT := n

# Generated Options
export A := $(PWD)
export NO_AXSTD := y
export AX_LIB := axfeat
export APP_FEATURES := qemu

# Disk Path
export DISK_PATH := $(abspath ./disk)

ifeq ($(MEMTRACK), y)
	APP_FEATURES += starry-api/memtrack
endif

IMG_URL = https://github.com/Starry-OS/rootfs/releases/download/20250917
IMG = rootfs-$(ARCH).img

img: build
	@echo ${DISK_PATH}
	@if [ ! -f $(IMG) ]; then \
		echo "Image not found, downloading..."; \
		curl -f -L $(IMG_URL)/$(IMG).xz -O; \
		xz -d $(IMG).xz; \
	fi
	@cp $(IMG) arceos/disk.img
	@-mkdir ./disk
	@-sudo mount arceos/disk.img ./disk
	@-sudo cp kallsyms ./disk/root/kallsyms
	@-sudo mkdir -p $(DISK_PATH)/musl
	@make -C user/musl all
	@sudo umount ./disk
	@rmdir ./disk
	@rm kallsyms

defconfig justrun clean:
	@make -C arceos $@

run:
	@make -C arceos justrun

build debug disasm: defconfig
	@make -C arceos $@

# Aliases
rv:
	$(MAKE) ARCH=riscv64 run

la:
	$(MAKE) ARCH=loongarch64 run

vf2:
	$(MAKE) ARCH=riscv64 APP_FEATURES=vf2 MYPLAT=axplat-riscv64-visionfive2 BUS=dummy build

.PHONY: build run justrun debug disasm clean img
