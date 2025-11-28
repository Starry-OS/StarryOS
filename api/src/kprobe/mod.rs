#[cfg(feature = "kprobe_test")]
pub mod kprobe_test;

use alloc::sync::Arc;

use axcpu::TrapFrame;
use kprobe::{
    Kprobe, Kretprobe, KretprobeBuilder, ProbeBuilder, ProbeManager, ProbePointList, PtRegs,
};
pub use starry_core::probe_aux::KprobeAuxiliary;

use crate::lock_api::KSpinNoPreempt;

pub type KernelKprobe = Kprobe<KSpinNoPreempt<()>, KprobeAuxiliary>;
pub type KernelKretprobe = Kretprobe<KSpinNoPreempt<()>, KprobeAuxiliary>;

pub static KPROBE_MANAGER: KSpinNoPreempt<ProbeManager<KSpinNoPreempt<()>, KprobeAuxiliary>> =
    KSpinNoPreempt::new(ProbeManager::new());
static KPROBE_POINT_LIST: KSpinNoPreempt<ProbePointList<KprobeAuxiliary>> =
    KSpinNoPreempt::new(ProbePointList::new());

/// Unregister a kprobe
pub fn unregister_kprobe(kprobe: Arc<KernelKprobe>) {
    let mut manager = KPROBE_MANAGER.lock();
    let mut kprobe_list = KPROBE_POINT_LIST.lock();
    kprobe::unregister_kprobe(&mut manager, &mut kprobe_list, kprobe);
}

/// Register a kprobe
pub fn register_kprobe(kprobe_builder: ProbeBuilder<KprobeAuxiliary>) -> Arc<KernelKprobe> {
    let mut manager = KPROBE_MANAGER.lock();
    let mut kprobe_list = KPROBE_POINT_LIST.lock();
    kprobe::register_kprobe(&mut manager, &mut kprobe_list, kprobe_builder)
}

/// unregister a kretprobe
pub fn unregister_kretprobe(kretprobe: Arc<KernelKretprobe>) {
    let mut manager = KPROBE_MANAGER.lock();
    let mut kprobe_list = KPROBE_POINT_LIST.lock();
    kprobe::unregister_kretprobe(&mut manager, &mut kprobe_list, kretprobe)
}

/// Register a kretprobe
pub fn register_kretprobe(
    kretprobe_builder: KretprobeBuilder<KSpinNoPreempt<()>>,
) -> Arc<KernelKretprobe> {
    let mut manager = KPROBE_MANAGER.lock();
    let mut kprobe_list = KPROBE_POINT_LIST.lock();
    kprobe::register_kretprobe(&mut manager, &mut kprobe_list, kretprobe_builder)
}

/// Handle kprobe from a breakpoint exception
pub fn break_kprobe_handler(frame: &mut TrapFrame) -> Option<()> {
    let mut manager = KPROBE_MANAGER.lock();
    let mut pt_regs = PtRegs::from(frame as &TrapFrame);
    let res = kprobe::kprobe_handler_from_break(&mut manager, &mut pt_regs);
    frame.update_from_ptregs(pt_regs);
    res
}

#[cfg(target_arch = "x86_64")]
/// Handle kprobe from a debug exception
pub fn debug_kprobe_handler(frame: &mut TrapFrame) -> Option<()> {
    let mut manager = KPROBE_MANAGER.lock();
    let mut pt_regs = PtRegs::from(frame as &TrapFrame);
    let res = kprobe::kprobe_handler_from_debug(&mut manager, &mut pt_regs);
    frame.update_from_ptregs(pt_regs);
    res
}
