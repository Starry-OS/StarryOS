//! The kprobe auxiliary implementation for the kernel.
use alloc::vec::Vec;

use axhal::{
    asm::flush_tlb,
    mem::{phys_to_virt, virt_to_phys},
    paging::{MappingFlags, PageSize},
};
use axmm::{
    backend::{alloc_frame, dealloc_frame},
    kernel_aspace,
};
use axtask::current_may_uninit;
use kprobe::{KprobeAuxiliaryOps, retprobe::RetprobeInstance};
use memory_addr::{MemoryAddr, PAGE_SIZE_4K, VirtAddr, VirtAddrRange, align_down_4k, align_up_4k};

use crate::{
    lock_api::KSpinNoPreempt,
    task::{AsThread, get_task},
};

static INSTANCE: KSpinNoPreempt<Vec<kprobe::retprobe::RetprobeInstance>> =
    KSpinNoPreempt::new(Vec::new());

/// The helper for kprobe auxiliary in the kernel.
#[derive(Debug)]
pub struct KprobeAuxiliary;

impl KprobeAuxiliary {
    fn set_writeable_for_address_user<F: FnOnce(*mut u8)>(
        address: usize,
        _len: usize,
        pid: i32,
        action: F,
    ) {
        let address = VirtAddr::from_usize(address);
        let task = get_task(pid as _).expect("Failed to get task for uprobe");
        let mut mm = task.as_thread().proc_data.aspace.lock();
        let flags = mm.memoryset().find(address).unwrap().flags();
        axlog::error!("Original flags for address {address:#x}: {flags:?}");

        // make text section writeable tmply
        mm.memoryset_mut()
            .find_mut(address)
            .expect("Failed to find memory area for uprobe")
            .set_flags(flags | MappingFlags::WRITE);

        // we use the page fault handler to trigger the COW handling.
        let res = mm.handle_page_fault(address, MappingFlags::WRITE);
        assert!(res);

        // restore the original permissions
        mm.protect(address.align_down_4k(), PAGE_SIZE_4K, flags)
            .unwrap();

        mm.memoryset_mut()
            .find_mut(address)
            .unwrap()
            .set_flags(flags);

        let (phy_addr, ..) = mm.page_table().query(address).unwrap();
        let kernel_virt_addr = axhal::mem::phys_to_virt(phy_addr);
        // in kernel space, the address is already mapped and writable
        action(kernel_virt_addr.as_mut_ptr());
        // action(address.as_mut_ptr());
        flush_tlb(Some(address));
    }

    fn set_writeable_for_address_kernel<F: FnOnce(*mut u8)>(address: usize, len: usize, action: F) {
        let addr = VirtAddr::from_usize(align_down_4k(address));
        let len = align_up_4k(len);

        let kspace = kernel_aspace();
        kspace
            .lock()
            .protect(
                addr,
                len,
                MappingFlags::READ | MappingFlags::EXECUTE | MappingFlags::WRITE,
            )
            .unwrap();
        action(address as *mut u8);
        // restore the original permission for text section
        kspace
            .lock()
            .protect(addr, len, MappingFlags::READ | MappingFlags::EXECUTE)
            .unwrap();
        flush_tlb(Some(VirtAddr::from_usize(address)));
    }
}

impl KprobeAuxiliaryOps for KprobeAuxiliary {
    fn copy_memory(src: *const u8, dst: *mut u8, len: usize, user_pid: Option<i32>) {
        if let Some(pid) = user_pid {
            let task = get_task(pid as _).expect("Failed to get task for uprobe");
            let mm = task.as_thread().proc_data.aspace.lock();

            let address = VirtAddr::from_ptr_of(src);
            let (phy_addr, ..) = mm.page_table().query(address).unwrap();
            let kernel_virt_addr = axhal::mem::phys_to_virt(phy_addr);
            unsafe {
                core::ptr::copy_nonoverlapping(kernel_virt_addr.as_ptr(), dst, len);
            }
        } else {
            unsafe {
                core::ptr::copy_nonoverlapping(src, dst, len);
            }
        }
    }

    fn set_writeable_for_address<F: FnOnce(*mut u8)>(
        address: usize,
        len: usize,
        user_pid: Option<i32>,
        action: F,
    ) {
        assert!(len < PAGE_SIZE_4K);
        if let Some(pid) = user_pid {
            Self::set_writeable_for_address_user(address, len, pid, action);
        } else {
            Self::set_writeable_for_address_kernel(address, len, action);
        }
    }

    fn alloc_kernel_exec_memory() -> *mut u8 {
        // ax_println!("alloc_executable_memory: layout={:?}", layout);
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

    fn free_kernel_exec_memory(ptr: *mut u8) {
        // ax_println!("dealloc_executable_memory: ptr={:?}", ptr);
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

    fn alloc_user_exec_memory<F: FnOnce(*mut u8)>(pid: Option<i32>, action: F) -> *mut u8 {
        let task = get_task(pid.unwrap() as _).expect("Failed to get task for uprobe");
        let mut mm = task.as_thread().proc_data.aspace.lock();

        let page_phy = alloc_frame(true, PageSize::Size4K).unwrap();

        let page_virt = phys_to_virt(page_phy);

        let start_vaddr = mm
            .find_free_area(
                mm.base(),
                PageSize::Size4K as _,
                VirtAddrRange::new(mm.base(), mm.end()),
            )
            .expect("Failed to find free area for uprobe");

        // The page has been mapped as read-write
        action(page_virt.as_mut_ptr());

        mm.map_linear(
            start_vaddr,
            page_phy,
            PAGE_SIZE_4K,
            MappingFlags::READ | MappingFlags::EXECUTE | MappingFlags::USER,
        )
        .unwrap();

        axlog::trace!(
            "Allocated user exec memory for pid {}: virt: {:#x}, phys: {:#x}",
            pid.unwrap(),
            start_vaddr.as_usize(),
            page_phy.as_usize()
        );

        start_vaddr.as_mut_ptr()
    }

    fn free_user_exec_memory(pid: Option<i32>, ptr: *mut u8) {
        let task = get_task(pid.unwrap() as _).expect("Failed to get task for uprobe");
        let mut mm = task.as_thread().proc_data.aspace.lock();

        let vaddr = VirtAddr::from_mut_ptr_of(ptr);
        let (paddr, ..) = mm.page_table().query(vaddr).unwrap();

        mm.unmap(vaddr, PAGE_SIZE_4K).unwrap();

        dealloc_frame(paddr, PageSize::Size4K);
    }

    fn insert_kretprobe_instance_to_task(instance: RetprobeInstance) {
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

    fn pop_kretprobe_instance_from_task() -> RetprobeInstance {
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
