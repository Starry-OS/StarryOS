#![no_std]
#![no_main]

use aya_ebpf::{helpers::bpf_probe_read, macros::kretprobe, programs::RetProbeContext};
use aya_log_ebpf::info;

#[kretprobe]
pub fn kret(ctx: RetProbeContext) -> u32 {
    match try_kret(ctx) {
        Ok(ret) => ret,
        Err(ret) => ret,
    }
}

// pub fn sys_getpid() -> AxResult<isize>;
fn try_kret(ctx: RetProbeContext) -> Result<u32, u32> {
    let pt_regs = unsafe { &*ctx.regs };
    let a0 = unsafe { bpf_probe_read(&pt_regs.a0) }.unwrap_or(u64::MAX);
    let a1 = unsafe { bpf_probe_read(&pt_regs.a1) }.unwrap_or(u64::MAX);
    info!(
        &ctx,
        "Function (sys_getpid) returned: a0={}, a1={}, ", a0, a1
    );
    Ok(0)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[unsafe(link_section = "license")]
#[unsafe(no_mangle)]
static LICENSE: [u8; 13] = *b"Dual MIT/GPL\0";
