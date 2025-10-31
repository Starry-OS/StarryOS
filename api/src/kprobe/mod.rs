#[cfg(feature = "kprobe_test")]
pub mod kprobe_test;

use alloc::{sync::Arc, vec::Vec};

use axcpu::TrapFrame;
use axhal::{
    mem::{phys_to_virt, virt_to_phys},
    paging::{MappingFlags, PageSize},
};
use axmm::{
    backend::{alloc_frame, dealloc_frame},
    kernel_aspace,
};
use axtask::current_may_uninit;
use kprobe::{
    Kprobe, KprobeAuxiliaryOps, KprobeBuilder, KprobeManager, KprobePointList, Kretprobe,
    KretprobeBuilder, PtRegs,
};
use memory_addr::{PAGE_SIZE_4K, VirtAddr, align_down_4k, align_up_4k};
use starry_core::task::AsThread;

use crate::lock_api::KSpinNoPreempt;

pub type KernelKprobe = Kprobe<KSpinNoPreempt<()>, KprobeAuxiliary>;
pub type KernelKretprobe = Kretprobe<KSpinNoPreempt<()>, KprobeAuxiliary>;

#[derive(Debug)]
pub struct KprobeAuxiliary;

impl KprobeAuxiliaryOps for KprobeAuxiliary {
    fn set_writeable_for_address(address: usize, len: usize, writable: bool) {
        // ax_println!(
        //     "set_writeable_for_address: address={:#x}, len={}, writable={}",
        //     address,
        //     len,
        //     writable
        // );
        assert!(len < PAGE_SIZE_4K);
        let kspace = kernel_aspace();
        let addr = VirtAddr::from_usize(align_down_4k(address));
        let len = align_up_4k(len);
        kspace
            .lock()
            .protect(
                addr,
                len,
                MappingFlags::READ
                    | MappingFlags::EXECUTE
                    | if writable {
                        MappingFlags::WRITE
                    } else {
                        MappingFlags::empty()
                    },
            )
            .unwrap();
    }

    fn alloc_executable_memory(layout: core::alloc::Layout) -> *mut u8 {
        // ax_println!("alloc_executable_memory: layout={:?}", layout);
        assert!(layout.size() < PAGE_SIZE_4K);
        let kspace = kernel_aspace();
        let mut guard = kspace.lock();
        let page_phy = alloc_frame(true, PageSize::Size4K).unwrap();
        let page_virt = phys_to_virt(page_phy);
        guard
            .protect(
                page_virt,
                PAGE_SIZE_4K,
                MappingFlags::READ | MappingFlags::WRITE | MappingFlags::EXECUTE,
            )
            .unwrap();
        page_virt.as_mut_ptr()
    }

    fn dealloc_executable_memory(ptr: *mut u8, layout: core::alloc::Layout) {
        // ax_println!("dealloc_executable_memory: ptr={:?}", ptr);
        assert!(layout.size() < PAGE_SIZE_4K);
        let kspace = kernel_aspace();
        let mut guard = kspace.lock();
        guard
            .protect(
                VirtAddr::from_mut_ptr_of(ptr),
                PAGE_SIZE_4K,
                MappingFlags::READ | MappingFlags::WRITE,
            )
            .unwrap();
        dealloc_frame(
            virt_to_phys(VirtAddr::from_mut_ptr_of(ptr)),
            PageSize::Size4K,
        );
    }

    fn insert_kretprobe_instance_to_task(instance: kprobe::KretprobeInstance) {
        let task = current_may_uninit();
        if let Some(task) = task {
            let thread = task.try_as_thread();
            if let Some(thread) = thread {
                let mut kretprobe_instances = thread.proc_data.kretprobe_instances.write();
                kretprobe_instances.push(instance);
                return;
            }
        }
        // If the current task is None, we can store it in a static variable
        let mut instances = INSTANCE.lock();
        instances.push(instance);
    }

    fn pop_kretprobe_instance_from_task() -> kprobe::KretprobeInstance {
        let task = current_may_uninit();
        if let Some(task) = task {
            let thread = task.try_as_thread();
            if let Some(thread) = thread {
                let mut kretprobe_instances = thread.proc_data.kretprobe_instances.write();
                return kretprobe_instances.pop().unwrap();
            }
        }
        // If the current task is None, we can pop it from the static variable
        let mut instances = INSTANCE.lock();
        instances.pop().unwrap()
    }
}

static INSTANCE: KSpinNoPreempt<Vec<kprobe::KretprobeInstance>> = KSpinNoPreempt::new(Vec::new());

pub static KPROBE_MANAGER: KSpinNoPreempt<KprobeManager<KSpinNoPreempt<()>, KprobeAuxiliary>> =
    KSpinNoPreempt::new(KprobeManager::new());
static KPROBE_POINT_LIST: KSpinNoPreempt<KprobePointList<KprobeAuxiliary>> =
    KSpinNoPreempt::new(KprobePointList::new());

/// Unregister a kprobe
pub fn unregister_kprobe(kprobe: Arc<KernelKprobe>) {
    let mut manager = KPROBE_MANAGER.lock();
    let mut kprobe_list = KPROBE_POINT_LIST.lock();
    kprobe::unregister_kprobe(&mut manager, &mut kprobe_list, kprobe);
}

/// Register a kprobe
pub fn register_kprobe(kprobe_builder: KprobeBuilder<KprobeAuxiliary>) -> Arc<KernelKprobe> {
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

pub fn run_all_kprobe(frame: &mut TrapFrame) -> Option<()> {
    let mut manager = KPROBE_MANAGER.lock();
    let mut pt_regs = PtRegs::from(frame as &TrapFrame);
    let res = kprobe::kprobe_handler_from_break(&mut manager, &mut pt_regs);
    frame.update_from_ptregs(pt_regs);
    res
}
