
# Makefile for building Linux kernel loadable modules
# =====================================================
# This Makefile is a translated version of the Rust builder (builder/src/main.rs)
# 
# Features:
#   - Builds Rust kernel modules for specified architectures
#   - Extracts object files from static libraries
#   - Links them into relocatable .ko (kernel object) files
#   - Supports multiple target architectures (x86_64, RISC-V, ARM, etc.)
#   - Verifies generated kernel modules
#
# Build Flow:
#   1. Compile module crate using cargo (--release, with custom target)
#   2. Extract object files from generated .a library using rust-ar
#   3. Link object files using ld with relocation enabled (-r flag)
#   4. Verify the resulting .ko file with file/readelf commands
#   5. Clean up temporary object files


# Build Options
KMOD_RUSTFLAGS := -C relocation-model=static

SELF_FILE := kmod.mk

ifeq ($(ARCH), x86_64)
  KMOD_RUSTFLAGS +=  -C code-model=large
else ifeq ($(ARCH), loongarch64)
  KMOD_RUSTFLAGS +=  -C code-model=large
endif

MODULE_PATHS ?= modules
KMOD_LINKER_SCRIPT ?= ${PWD}/kmod-linker.ld

export KMOD_RUSTFLAGS
export KMOD_LINKER_SCRIPT


# Get list of modules (directories in MODULE_PATHS)
MODULES := $(shell if [ -d $(MODULE_PATHS) ]; then ls -d $(MODULE_PATHS)/*/ 2>/dev/null | xargs -I {} basename {}; fi)


.PHONY: all clean modules $(MODULES) list-modules help

# Default target
all: modules

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
	@echo "  MODULE_PATHS     Module search path (default: modules)"
	@echo "  KMOD_LINKER_SCRIPT    Linker script path (default: linker.ld)"
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
	KMOD=y APP=$(abspath $(MODULE_PATHS)/$@) make -C arceos build_ko

copy_modules:
	@for module in $(MODULES); do \
		echo "Copying module: $$module"; \
		sudo cp $(MODULE_PATHS)/$$module/*.ko ./disk/root/modules/$$module.ko; \
	done

clean:
	@echo "Cleaning build artifacts..."
	@rm -rf $(BUILD_DIR)
	@echo "Clean complete"

rebuild: clean all
	@echo "Rebuild complete"

show-config:
	@echo "Build Configuration:"
	@echo "  MODULE_PATHS: $(MODULE_PATHS)"
	@echo "  KMOD_LINKER_SCRIPT: $(KMOD_LINKER_SCRIPT)"
	@echo "  Available modules: $(MODULES)"

