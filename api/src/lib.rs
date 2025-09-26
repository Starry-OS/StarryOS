#![no_std]
#![feature(likely_unlikely)]
#![feature(bstr)]
#![feature(maybe_uninit_slice)]
#![allow(missing_docs)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use axerrno::{LinuxError, LinuxResult};
#[macro_use]
extern crate axlog;

extern crate alloc;

mod exception;
pub mod file;
pub mod io;
pub mod kprobe;
mod lock_api;
pub mod mm;
pub mod signal;
pub mod socket;
pub mod syscall;
pub mod task;
pub mod terminal;
pub mod time;
pub mod vfs;

/// Initialize.
pub fn init() {
    #[cfg(feature = "kprobe_test")]
    kprobe::kprobe_test::kprobe_test();

    if axconfig::plat::CPU_NUM > 1 {
        panic!("SMP is not supported");
    }
    info!("Initialize VFS...");
    vfs::mount_all().expect("Failed to mount vfs");

    info!("Initialize /proc/interrupts...");
    axtask::register_timer_callback(|_| {
        time::inc_irq_cnt();
    });

    info!("Initialize alarm...");
    starry_core::time::spawn_alarm_task();
}

pub fn kernel_catch_unwind<R, F: FnOnce() -> R>(f: F) -> LinuxResult<R> {
    let res = unwinding::panic::catch_unwind(f);
    match res {
        Ok(r) => Ok(r),
        Err(e) => {
            ax_println!("Catch Unwind Error: {:?}", e);
            Err(LinuxError::EAGAIN)
        }
    }
}
