use alloc::sync::Arc;

use axcpu::TrapFrame;
use axtask::current;
use kprobe::{ProbeBuilder, PtRegs, Uprobe};
use starry_core::{lock_api::KSpinNoPreempt, probe_aux::KprobeAuxiliary, task::AsThread};

/// The uprobe type for the kernel.
pub type KernelUprobe = Uprobe<KSpinNoPreempt<()>, KprobeAuxiliary>;

/// Register a uprobe
pub fn register_uprobe(uprobe_builder: ProbeBuilder<KprobeAuxiliary>) -> Arc<KernelUprobe> {
    let current = current();
    let mut uprobe_manager = current.as_thread().proc_data.uprobe_manager.lock();
    let mut uprobe_point_list = current.as_thread().proc_data.uprobe_point_list.lock();
    kprobe::register_uprobe(&mut uprobe_manager, &mut uprobe_point_list, uprobe_builder)
}

/// Unregister a uprobe
pub fn unregister_uprobe(uprobe: Arc<KernelUprobe>) {
    let current = current();
    let mut uprobe_manager = current.as_thread().proc_data.uprobe_manager.lock();
    let mut uprobe_point_list = current.as_thread().proc_data.uprobe_point_list.lock();
    kprobe::unregister_uprobe(&mut uprobe_manager, &mut uprobe_point_list, uprobe);
}

/// Handle kprobe from a breakpoint exception
pub fn break_uprobe_handler(frame: &mut TrapFrame) -> Option<()> {
    let current = current();
    let mut uprobe_manager = current.as_thread().proc_data.uprobe_manager.lock();
    let mut pt_regs = PtRegs::from(frame as &TrapFrame);
    let res = kprobe::uprobe_handler_from_break(&mut uprobe_manager, &mut pt_regs);
    frame.update_from_ptregs(pt_regs);
    res
}

#[cfg(target_arch = "x86_64")]
/// Handle kprobe from a debug exception
pub fn debug_uprobe_handler(frame: &mut TrapFrame) -> Option<()> {
    let current = current();
    let mut uprobe_manager = current.as_thread().proc_data.uprobe_manager.lock();
    let mut pt_regs = PtRegs::from(frame as &TrapFrame);
    let res = kprobe::uprobe_handler_from_debug(&mut uprobe_manager, &mut pt_regs);
    frame.update_from_ptregs(pt_regs);
    res
}
