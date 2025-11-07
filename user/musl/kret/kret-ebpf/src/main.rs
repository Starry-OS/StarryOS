#![no_std]
#![no_main]

use aya_ebpf::{macros::kretprobe, programs::RetProbeContext};
use aya_log_ebpf::info;

#[kretprobe]
pub fn kret(ctx: RetProbeContext) -> u32 {
    match try_kret(ctx) {
        Ok(ret) => ret,
        Err(ret) => ret,
    }
}

#[cfg(feature = "riscv64")]
pub fn get_arg0(ctx: &RetProbeContext) -> u64 {
    let pt_regs = unsafe { &*ctx.regs };
    pt_regs.a0 as u64
}

#[cfg(feature = "x86_64")]
pub fn get_arg0(cxt: &RetProbeContext) -> u64 {
    // first arg -> rdi
    // second arg -> rsi
    // third arg -> rdx
    // four arg -> rcx
    let pt_regs = unsafe { &*cxt.regs };
    pt_regs.rdi as u64
}

#[cfg(feature = "loongarch64")]
pub fn get_arg0(ctx: &RetProbeContext) -> u64 {
    let pt_regs = unsafe { &*ctx.regs };
    pt_regs.regs[4] as u64
}

#[cfg(feature = "riscv64")]
pub fn get_arg1(ctx: &RetProbeContext) -> u64 {
    let pt_regs = unsafe { &*ctx.regs };
    pt_regs.a1 as u64
}

#[cfg(feature = "x86_64")]
pub fn get_arg1(cxt: &RetProbeContext) -> u64 {
    // first arg -> rdi
    // second arg -> rsi
    // third arg -> rdx
    // four arg -> rcx
    let pt_regs = unsafe { &*cxt.regs };
    pt_regs.rsi as u64
}

#[cfg(feature = "loongarch64")]
pub fn get_arg1(ctx: &RetProbeContext) -> u64 {
    let pt_regs = unsafe { &*ctx.regs };
    pt_regs.regs[5] as u64
}

// pub fn sys_getpid() -> AxResult<isize>;
fn try_kret(ctx: RetProbeContext) -> Result<u32, u32> {
    let a0 = get_arg0(&ctx) as u64;
    let a1 = get_arg1(&ctx) as u64;
    // let a0 = unsafe { bpf_probe_read(&pt_regs.a0) }.unwrap_or(u64::MAX);
    // let a1 = unsafe { bpf_probe_read(&pt_regs.a1) }.unwrap_or(u64::MAX);
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
