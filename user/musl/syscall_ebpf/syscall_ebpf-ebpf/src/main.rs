#![no_std]
#![no_main]

use aya_ebpf::{
    helpers::bpf_ktime_get_ns,
    macros::{kprobe, map},
    maps::HashMap,
    programs::ProbeContext,
};
use aya_log_ebpf::info;

#[kprobe]
pub fn syscall_ebpf(ctx: ProbeContext) -> u32 {
    try_syscall_ebpf(ctx).unwrap_or_else(|ret| ret)
}

#[cfg(feature = "riscv64")]
pub fn get_arg0(ctx: &ProbeContext) -> u64 {
    let pt_regs = unsafe { &*ctx.regs };
    pt_regs.a0 as u64
}

#[cfg(feature = "x86_64")]
pub fn get_arg0(cxt: &ProbeContext) -> u64 {
    // first arg -> rdi
    // second arg -> rsi
    // third arg -> rdx
    // four arg -> rcx
    let pt_regs = unsafe { &*cxt.regs };
    pt_regs.rdi as u64
}

#[cfg(feature = "loongarch64")]
pub fn get_arg0(ctx: &ProbeContext) -> u64 {
    let pt_regs = unsafe { &*ctx.regs };
    pt_regs.regs[4] as u64
}

fn try_syscall_ebpf(ctx: ProbeContext) -> Result<u32, u32> {
    let syscall_num = get_arg0(&ctx) as usize;
    if syscall_num != 1 {
        unsafe {
            if let Some(v) = SYSCALL_LIST.get(&(syscall_num as u32)) {
                let new_v = *v + 1;
                SYSCALL_LIST
                    .insert(&(syscall_num as u32), &new_v, 0)
                    .unwrap();
            } else {
                SYSCALL_LIST.insert(&(syscall_num as u32), &1, 0).unwrap();
            }
        }
        let time = unsafe { bpf_ktime_get_ns() };
        info!(&ctx, "[{}] invoke syscall {}", time, syscall_num);
    }
    Ok(0)
}

#[map]
static SYSCALL_LIST: HashMap<u32, u32> = HashMap::<u32, u32>::with_max_entries(1024, 0);

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // we need use this because the verifier will forbid loop
    unsafe { core::hint::unreachable_unchecked() }
    // loop{}
}

#[unsafe(link_section = "license")]
#[unsafe(no_mangle)]
static LICENSE: [u8; 13] = *b"Dual MIT/GPL\0";
