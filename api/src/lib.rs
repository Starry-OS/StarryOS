#![no_std]
#![feature(likely_unlikely)]
#![feature(bstr)]
#![feature(maybe_uninit_slice)]
#![allow(missing_docs)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

#[macro_use]
extern crate axlog;

extern crate alloc;
#[cfg(not(target_arch = "loongarch64"))]
use axerrno::{LinuxError, LinuxResult};

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
pub mod tracepoint;
pub mod vfs;

/// Initialize.
pub fn init() {
    #[cfg(feature = "kprobe_test")]
    kprobe::kprobe_test::kprobe_test();

    tracepoint::tracepoint_init();

    if axconfig::plat::CPU_NUM > 1 {
        panic!("SMP is not supported");
    }
    info!("Initialize VFS...");
    vfs::mount_all().expect("Failed to mount vfs");

    info!("Initialize /proc/interrupts...");
    axtask::register_timer_callback(|_| {
        time::inc_irq_cnt();
    });

    #[cfg(not(target_arch = "loongarch64"))]
    test_unwind();

    info!("Initialize alarm...");
    starry_core::time::spawn_alarm_task();
}

#[cfg(not(target_arch = "loongarch64"))]
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

#[cfg(not(target_arch = "loongarch64"))]
pub fn test_unwind() {
    struct UnwindTest;
    impl Drop for UnwindTest {
        fn drop(&mut self) {
            ax_println!("Drop UnwindTest");
        }
    }
    let res1 = unwinding::panic::catch_unwind(|| {
        let _unwind_test = UnwindTest;
        ax_println!("Test panic...");
        panic!("Test panic");
    });
    assert!(res1.is_err());
    let res2 = unwinding::panic::catch_unwind(|| {
        let _unwind_test = UnwindTest;
        ax_println!("Test no panic...");
        0
    });
    assert!(res2.is_ok());
}
