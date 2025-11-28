#[cfg(target_arch = "x86_64")]
use axcpu::trap::DEBUG_HANDLER;
use axcpu::{TrapFrame, trap::BREAK_HANDLER};
use linkme::distributed_slice;
#[distributed_slice(BREAK_HANDLER)]
static BREAK_HANDLER_F: fn(&mut TrapFrame, arg: u64) -> bool = kernel_ebreak_handler;

/// The kernel ebreak handler.
pub fn kernel_ebreak_handler(tf: &mut TrapFrame, _arg: u64) -> bool {
    let res = crate::kprobe::break_kprobe_handler(tf);
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
static DEBUG_HANDLER_F: fn(&mut TrapFrame) -> bool = kernel_debug_handler;

#[cfg(target_arch = "x86_64")]
/// The kernel debug handler.
pub fn kernel_debug_handler(tf: &mut TrapFrame) -> bool {
    let _res = crate::kprobe::debug_kprobe_handler(tf);

    true
}

/// The user ebreak handler.
pub fn user_ebreak_handler(tf: &mut TrapFrame, _arg: u64) -> bool {
    let res = crate::uprobe::break_uprobe_handler(tf);
    if res.is_some() {
        // if uprobe is hit, the spec will be updated in uprobe_handler
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
/// The user debug handler.
pub fn user_debug_handler(tf: &mut TrapFrame) -> bool {
    let _res = crate::uprobe::debug_uprobe_handler(tf);
    true
}
