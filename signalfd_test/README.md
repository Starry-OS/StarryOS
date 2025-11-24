## signalfd_test

This crate is a tiny user-space program used to validate the fix for StarryOS issue
[#15](https://github.com/Starry-OS/StarryOS/issues/15), where the `signalfd4` system
call previously returned a dummy file descriptor that panicked on read.

### Build

```bash
cargo build -p signalfd_test --release --target riscv64gc-unknown-linux-musl
```

The binary is statically linked when the `riscv64-linux-musl` toolchain is installed
under `/opt/musl`. Adjust `CARGO_TARGET_*_LINKER` if your toolchain lives elsewhere.

### Deploy into the StarryOS rootfs

```bash
sudo mount -o loop arceos/disk.img /mnt
sudo cp target/riscv64gc-unknown-linux-musl/release/signalfd_test /mnt/root/
sudo chmod +x /mnt/root/signalfd_test
sudo umount /mnt
```

### Run inside QEMU

```text
make run
# inside the guest shell
starry:~# ./signalfd_test
signalfd created: fd = 3
read 128 bytes from signalfd
```

A successful run confirms that `signalfd4` now returns a real file descriptor backed
by the kernel implementation, and that reading from it yields the expected
128-byte `signalfd_siginfo` structure without panicking.
