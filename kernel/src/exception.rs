#[cfg(target_arch = "x86_64")]
use axhal::trap::DEBUG_HANDLER;
use axhal::{context::TrapFrame, trap::BREAK_HANDLER};
use linkme::distributed_slice;

#[distributed_slice(BREAK_HANDLER)]
static BREAK_HANDLER_F: fn(&mut TrapFrame) -> bool = kernel_ebreak_handler;

/// The kernel ebreak handler.
pub fn kernel_ebreak_handler(tf: &mut TrapFrame) -> bool {
    let res = crate::kprobe::break_kprobe_handler(tf);
    if res.is_some() {
        // if kprobe is hit, the spec will be updated in kprobe_handler
        return true;
    }
    false
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
/// Returns true if the ebreak is handled by uprobe, false otherwise.
pub fn user_ebreak_handler(tf: &mut TrapFrame) -> bool {
    let res = crate::uprobe::break_uprobe_handler(tf);
    if res.is_some() {
        // if uprobe is hit, the pc will be updated in uprobe_handler
        return true;
    }
    // For x86_64, pc will automatically point to the next instruction after the int3 instruction, so we don't need to manually update it here.
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
    false
}

#[cfg(target_arch = "x86_64")]
/// The user debug handler.
pub fn user_debug_handler(tf: &mut TrapFrame) {
    let _res = crate::uprobe::debug_uprobe_handler(tf);
}
