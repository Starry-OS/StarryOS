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

# Default values
RUSTFLAGS := -C relocation-model=static

ifeq ($(ARCH), x86_64)
  TARGET := x86_64-unknown-none
  RUSTFLAGS +=  -C code-model=large
else ifeq ($(ARCH), aarch64)
  TARGET := aarch64-unknown-none-softfloat
else ifeq ($(ARCH), riscv64)
  TARGET := riscv64gc-unknown-none-elf
else ifeq ($(ARCH), loongarch64)
  TARGET := loongarch64-unknown-none-softfloat
  RUSTFLAGS +=  -C code-model=large
else
  $(error "ARCH" must be one of "x86_64", "riscv64", "aarch64" or "loongarch64")
endif

MODULE_PATHS ?= modules
LINKER_SCRIPT ?= linker.ld

build_args := \
  -Zunstable-options \
  -Zbuild-std=core,alloc,compiler_builtins \
  -Zbuild-std-features=compiler-builtins-mem \
  --release \
  --target $(TARGET)

LINK_ARGS := \
  -C link-arg=-T$(LD_SCRIPT) \
  -C link-arg=-znostart-stop-gc \
  -C no-redzone=y

LD_COMMAND := ld.lld

# Get list of modules (directories in MODULE_PATHS)
MODULES := $(shell if [ -d $(MODULE_PATHS) ]; then ls -d $(MODULE_PATHS)/*/ 2>/dev/null | xargs -I {} basename {}; fi)

# Build output directory
BUILD_DIR := target
MODULE_BUILD_DIR := $(BUILD_DIR)/$(TARGET)/release


ifeq ($(MEMTRACK), y)
	APP_FEATURES += starry-api/memtrack
endif

IMG_URL = https://github.com/Starry-OS/rootfs/releases/download/20250917
IMG = rootfs-$(ARCH).img

img: kmod build
	@echo ${DISK_PATH}
	@if [ ! -f $(IMG) ]; then \
		echo "Image not found, downloading..."; \
		curl -f -L $(IMG_URL)/$(IMG).xz -O; \
		xz -d $(IMG).xz; \
	fi
	@if [ ! -f arceos/disk_$(ARCH).img ]; then \
		echo "Copying image to arceos/disk.img..."; \
		cp $(IMG) arceos/disk_$(ARCH).img; \
	fi
	@-mkdir ./disk
	@-sudo mount arceos/disk_$(ARCH).img ./disk
	@sudo cp kallsyms ./disk/root/kallsyms
# Copy all modules to disk
	@make copy_modules
	@-sudo mkdir -p $(DISK_PATH)/musl
	@make -C user/musl all
	@sudo umount ./disk
	@rmdir ./disk
	@rm -f arceos/disk.img
	@ln -s $(abspath arceos/disk_$(ARCH).img) arceos/disk.img
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



.PHONY: all  modules $(MODULES) list-modules help

# Default target
kmod: modules

# List available modules
list-modules:
	@echo "Available modules:"
	@for module in $(MODULES); do \
		echo "  - $$module"; \
	done

# Help target
help:
	@echo "Usage: make [target] [VAR=value]"
	@echo ""
	@echo "Targets:"
	@echo "  all              Build all modules (default)"
	@echo "  modules          Build all modules"
	@echo "  <module_name>    Build specific module"
	@echo "  list-modules     List available modules"
	@echo "  clean            Clean build artifacts"
	@echo "  help             Show this help message"
	@echo ""
	@echo "Variables:"
	@echo "  TARGET           Target triple (default: x86_64-unknown-none)"
	@echo "  MODULE_PATHS     Module search path (default: modules)"
	@echo "  LINKER_SCRIPT    Linker script path (default: linker.ld)"
	@echo ""
	@echo "Examples:"
	@echo "  make                              # Build all modules"
	@echo "  make hello                        # Build hello module"
	@echo "  make TARGET=riscv64gc-unknown-none-elf  # Build for RISC-V"

# Build all modules
modules: $(MODULES)

# Individual module target
$(MODULES):
	@echo "Building module: $@"
	cd $(MODULE_PATHS)/$@ && RUSTFLAGS="$(RUSTFLAGS)" cargo build $(build_args)
	@$(MAKE) process-module-library MODULE_NAME=$@ TARGET=$(TARGET) LD_COMMAND=$(LD_COMMAND)
	@$(MAKE) verify-kernel-module KO_PATH=$(BUILD_DIR)/$@/$@.ko

copy_modules:
	@for module in $(MODULES); do \
		$(MAKE) copy_module MODULE_NAME=$$module; \
	done

copy_module: 
	@echo "Copying module: $(MODULE_NAME) to disk"
	@-sudo mkdir -p ./disk/root/modules
	@sudo cp $(BUILD_DIR)/$(MODULE_NAME)/$(MODULE_NAME).ko ./disk/root/modules/$(MODULE_NAME).ko

.PHONY: process-module-library
process-module-library:
	@echo "Processing module library: $(MODULE_NAME)"
	@bash build_module.sh $(MODULE_NAME) $(TARGET) $(MODULE_BUILD_DIR) $(BUILD_DIR) $(LD_COMMAND)

.PHONY: verify-kernel-module
verify-kernel-module:
	@echo "Verifying kernel module: $(KO_PATH)"
	@if [ ! -f "$(KO_PATH)" ]; then \
		echo "Error: Kernel module file not found: $(KO_PATH)"; \
		exit 1; \
	fi
	@size=$$(stat -f%z "$(KO_PATH)" 2>/dev/null || stat -c%s "$(KO_PATH)" 2>/dev/null || echo 0); \
	if [ "$$size" -eq 0 ]; then \
		echo "Error: Kernel module file is empty"; \
		exit 1; \
	fi; \
	echo "Module size: $$size bytes"
	@if command -v file >/dev/null 2>&1; then \
		echo "File type:"; \
		file "$(KO_PATH)"; \
	fi
	@if command -v readelf >/dev/null 2>&1; then \
		echo "Module sections:"; \
		readelf -S "$(KO_PATH)" | grep -E "^\s+\[|PROGBITS|NOBITS"; \
	fi


# clean:
# 	@echo "Cleaning build artifacts..."
# 	@rm -rf $(BUILD_DIR)
# 	@echo "Clean complete"

# rebuild: clean all
# 	@echo "Rebuild complete"

show-config:
	@echo "Build Configuration:"
	@echo "  TARGET: $(TARGET)"
	@echo "  MODULE_PATHS: $(MODULE_PATHS)"
	@echo "  LINKER_SCRIPT: $(LINKER_SCRIPT)"
	@echo "  LD_COMMAND: $(LD_COMMAND)"
	@echo "  Available modules: $(MODULES)"


.PHONY: build run justrun debug disasm clean img kmod
