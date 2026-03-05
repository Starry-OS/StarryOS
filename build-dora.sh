#!/bin/bash
set -e
pushd ~/Projects/dora
./build-static.sh
popd

cp rootfs-riscv64.img rootfs-riscv64-dora.img
sudo mount rootfs-riscv64-dora.img mnt
sudo mv ~/Projects/dora/dora mnt/root/
sudo umount mnt
cp rootfs-riscv64-dora.img arceos/disk.img
