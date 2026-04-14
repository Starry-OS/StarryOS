TARGET :=

ifeq ($(ARCH), x86_64)
  TARGET := x86_64-unknown-linux-musl
else ifeq ($(ARCH), aarch64)
  TARGET := aarch64-unknown-linux-musl
else ifeq ($(ARCH), riscv64)
  TARGET := riscv64gc-unknown-linux-musl
else ifeq ($(ARCH), loongarch64)
  TARGET := loongarch64-unknown-linux-musl
else
  $(error "ARCH" must be one of "x86_64", "riscv64", "aarch64" or "loongarch64")
endif

ifeq ($(LOCAL_MNT), /tmp)
  $(warning "LOCAL_MNT" is not set, the default value is /tmp, which is not suitable for storing the built apps. Please set LOCAL_MNT to a proper path.)
endif

compile := cargo build --release --target=$(TARGET)
mode := release
target := $(TARGET_DIR)/$(TARGET)/$(mode)


write:
	@echo "Moving apps to $(LOCAL_MNT)/musl: $(APPS)"
	@$(foreach app, $(APPS), (sudo install -D $(target)/$(app) $(LOCAL_MNT)/musl/$(app););)

build:
	@echo "Building apps($(ARCH)): $(APPS)"
	@$(foreach app, $(APPS), (cd $(app) && $(compile) --target-dir $(TARGET_DIR));)

APPS = \
	async_test \
	syscall_ebpf \
	kret \
	rawtp \
	mytrace \
	upb \
	upb2

.PHONY: build write
	
