# Build Options
export RUSTC_BOOTSTRAP := 1
export ARCH := aarch64
export LOG := warn
export BACKTRACE := y
export MEMTRACK := n

# QEMU Options
export BLK := y
export NET := y
export VSOCK := y
export MEM := 1G
export ICOUNT := n
export SMP := 4

# Generated Options
export A := $(PWD)
export NO_AXSTD := y
export AX_LIB := axfeat
export APP_FEATURES := qemu

ifeq ($(MEMTRACK), y)
	APP_FEATURES += starry-api/memtrack
endif

IMG_URL = https://github.com/Starry-OS/rootfs/releases/download/20250917
IMG = rootfs-$(ARCH).img

img:
	@if [ ! -f $(IMG) ]; then \
		echo "Image not found, downloading..."; \
		curl -f -L $(IMG_URL)/$(IMG).xz -O; \
		xz -d $(IMG).xz; \
	fi
	@cp $(IMG) arceos/disk.img

defconfig justrun clean:
	@make -C arceos $@

build run debug disasm: defconfig
	@make -C arceos $@

# Aliases
rv:
	$(MAKE) ARCH=riscv64 run

la:
	$(MAKE) ARCH=loongarch64 run

vf2:
	$(MAKE) ARCH=riscv64 APP_FEATURES=vf2 MYPLAT=axplat-riscv64-visionfive2 BUS=dummy build

crosvm:
	$(MAKE) --debug=v ARCH=aarch64 APP_FEATURES=crosvm MYPLAT=axplat-aarch64-crosvm-virt BUS=pci LOG=warn build

dice:
	$(MAKE) --debug=v ARCH=aarch64 APP_FEATURES=dice MYPLAT=axplat-aarch64-crosvm-virt BUS=pci LOG=warn build
.PHONY: build run justrun debug disasm clean
