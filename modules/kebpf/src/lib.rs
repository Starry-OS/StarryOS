#![no_std]
extern crate alloc;

use kmod::{exit_fn, init_fn, module};


#[init_fn]
pub fn kebpf_init() -> i32 {
    axlog::ax_println!("Hello, eBPF Kernel Module!");
    0
}

#[exit_fn]
fn kebpf_exit() {
    axlog::ax_println!("Goodbye, eBPF Kernel Module!");
}

module!(
    name: "kebpf",
    license: "GPL",
    description: "kernel eBPF module",
    version: "0.1.0",
);
