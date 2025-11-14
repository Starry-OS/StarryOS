use axcpu::trap::BREAK_HANDLER;
#[cfg(target_arch = "x86_64")]
use axcpu::trap::DEBUG_HANDLER;
use axhal::context::TrapFrame;
use linkme::distributed_slice;

#[distributed_slice(BREAK_HANDLER)]
static BREAK_HANDLER_F: fn(&mut TrapFrame, arg: u64) -> bool = ebreak_handler;

// break 异常处理
#[allow(static_mut_refs)]
pub fn ebreak_handler(tf: &mut TrapFrame, _arg: u64) -> bool {
    // ax_println!("ebreak_handler from kernel");
    let res = crate::kprobe::kernel_break_kprobe(tf);
    if res.is_some() {
        // if kprobe is hit, the spec will be updated in kprobe_handler
        return true;
    }
    #[cfg(target_arch = "riscv64")]
    {
        tf.sepc += 2;
    }
    #[cfg(target_arch = "loongarch64")]
    {
        tf.era += 4;
    }
    #[cfg(target_arch = "aarch64")]
    {
        tf.elr += 4;
    }
    true
}

#[cfg(target_arch = "x86_64")]
#[distributed_slice(DEBUG_HANDLER)]
static DEBUG_HANDLER_F: fn(&mut TrapFrame) -> bool = debug_handler;

#[cfg(target_arch = "x86_64")]
pub fn debug_handler(tf: &mut TrapFrame) -> bool {
    let _res = crate::kprobe::kernel_debug_kprobe(tf);
    true
}
