TARGET = riscv64gc-unknown-linux-musl
CC = riscv64-linux-musl-gcc
ifeq ($(ARCH), x86_64)
  TARGET := x86_64-unknown-linux-musl
  CC := x86_64-linux-musl-gcc
else ifeq ($(ARCH), aarch64)
  TARGET := aarch64-unknown-linux-musl
  CC := aarch64-linux-musl-gcc
else ifeq ($(ARCH), riscv64)
  TARGET := riscv64gc-unknown-linux-musl
  CC := riscv64-linux-musl-gcc
else ifeq ($(ARCH), loongarch64)
  TARGET := loongarch64-unknown-linux-musl
  CC := loongarch64-linux-musl-gcc
else
  $(error "ARCH" must be one of "x86_64", "riscv64", "aarch64" or "loongarch64")
endif

compile := cargo build --release --target=$(TARGET)
mode := release
target := ../../target/$(TARGET)/$(mode)/



all:build
	@echo "Moving apps to ../diskfs/bin"
	$(foreach app, $(BUILD_CRATES), (sudo cp $(target)$(app) $(DISK_PATH)/musl/$(app););)
	$(foreach app, $(BUILD_EBPF_CRATES), (sudo cp $(target)$(app) $(DISK_PATH)/musl/$(app););)


build:
	@echo "Building apps"
	$(foreach app, $(BUILD_CRATES), ($(compile) -p $(app));)
	$(foreach app, $(BUILD_EBPF_CRATES), (cd $(app) && $(compile) --target-dir ../../../target);)


BUILD_CRATES = \
	async_test

BUILD_EBPF_CRATES = \
	mytrace

ifeq ($(ARCH), riscv64)
	BUILD_EBPF_CRATES += syscall_ebpf
endif