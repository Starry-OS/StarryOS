#![no_std]
#![feature(likely_unlikely)]
#![feature(bstr)]
#![allow(missing_docs)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

#[macro_use]
extern crate axlog;

extern crate alloc;

pub mod file;
pub mod io;
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
    info!("Initialize VFS...");
    vfs::mount_all().expect("Failed to mount vfs");

    // Enable user-space access to timer counter registers on aarch64
    #[cfg(target_arch = "aarch64")]
    starry_vdso::vdso::enable_cntvct_access();

    info!("Initialize vDSO data...");
    starry_vdso::vdso::init_vdso_data();

    info!("Initialize /proc/interrupts...");
    axtask::register_timer_callback(|_| {
        time::inc_irq_cnt();
        starry_vdso::vdso::update_vdso_data();
    });

    info!("Initialize alarm...");
    starry_core::time::spawn_alarm_task();
}
