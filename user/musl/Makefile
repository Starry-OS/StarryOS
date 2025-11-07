TARGET = riscv64gc-unknown-linux-musl
CC = riscv64-linux-musl-gcc
TARGET_FEATURE = riscv64

ifeq ($(ARCH), x86_64)
  TARGET := x86_64-unknown-linux-musl
  CC := x86_64-linux-musl-gcc
  TARGET_FEATURE = x86_64
else ifeq ($(ARCH), aarch64)
  TARGET := aarch64-unknown-linux-musl
  CC := aarch64-linux-musl-gcc
  TARGET_FEATURE = aarch64
else ifeq ($(ARCH), riscv64)
  TARGET := riscv64gc-unknown-linux-musl
  CC := riscv64-linux-musl-gcc
  TARGET_FEATURE = riscv64
else ifeq ($(ARCH), loongarch64)
  TARGET := loongarch64-unknown-linux-musl
  CC := loongarch64-linux-musl-gcc
  TARGET_FEATURE = loongarch64
else
  $(error "ARCH" must be one of "x86_64", "riscv64", "aarch64" or "loongarch64")
endif

compile := cargo build --release --target=$(TARGET)
mode := release
target := ../../target/$(TARGET)/$(mode)/



all: build
	@echo "Moving apps to ../diskfs/bin"
	$(foreach app, $(BUILD_CRATES), (sudo cp $(target)$(app) $(DISK_PATH)/musl/$(app););)
	$(foreach app, $(BUILD_EBPF_CRATES), (sudo cp $(target)$(app) $(DISK_PATH)/musl/$(app););)
	$(foreach app, $(BUILD_EBPF_CRATES_ARCH), (sudo cp $(target)$(app) $(DISK_PATH)/musl/$(app););)


build:
	@echo "Building apps"
	$(foreach app, $(BUILD_CRATES), ($(compile) -p $(app));)
	$(foreach app, $(BUILD_EBPF_CRATES), (cd $(app) && $(compile) --target-dir ../../../target);)
	$(foreach app, $(BUILD_EBPF_CRATES_ARCH), (cd $(app) && $(compile) --features=$(TARGET_FEATURE) --target-dir ../../../target);)


BUILD_CRATES = \
	async_test

BUILD_EBPF_CRATES = \
	rawtp \
	mytrace

BUILD_EBPF_CRATES_ARCH = \
	syscall_ebpf \
	kret
