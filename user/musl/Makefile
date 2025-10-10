TARGET = riscv64gc-unknown-linux-musl
compile := cargo build --release  --target=$(TARGET)
mode := release
target := ../../target/$(TARGET)/$(mode)/



all:build
	@echo "Moving apps to ../diskfs/bin"
	$(foreach app, $(BUILD_CRATES), (sudo cp $(target)$(app) $(DISK_PATH)/musl/$(app););)
	@sudo cp $(target)syscall_ebpf $(DISK_PATH)/musl/syscall_ebpf


build:
	@echo "Building apps"
	$(foreach app, $(BUILD_CRATES), ($(compile) -p $(app));)
	cd syscall_ebpf && $(compile) --target-dir ../../../target


BUILD_CRATES := \
	async_test