mod shim;
use alloc::{
    boxed::Box,
    collections::btree_map::BTreeMap,
    ffi::CString,
    string::{String, ToString},
};

use axalloc::UsageKind;
use axerrno::{AxError, AxResult};
use axhal::{
    asm::flush_tlb,
    mem::phys_to_virt,
    paging::{MappingFlags, PageSize},
};
use axmm::{
    backend::{alloc_frames, dealloc_frames},
    kernel_aspace,
};
use kmod_loader::{KernelModuleHelper, ModuleLoader, ModuleOwner, SectionMemOps};
use kspin::SpinNoPreempt;
use memory_addr::{PAGE_SIZE_4K, PhysAddr, VirtAddr};

pub struct KmodHelper;

fn section_perms_to_mapping_flags(perms: kmod_loader::SectionPerm) -> MappingFlags {
    let mut flags = MappingFlags::empty();
    if perms.contains(kmod_loader::SectionPerm::READ) {
        flags |= MappingFlags::READ;
    }
    if perms.contains(kmod_loader::SectionPerm::WRITE) {
        flags |= MappingFlags::WRITE;
    }
    flags |= MappingFlags::WRITE;
    if perms.contains(kmod_loader::SectionPerm::EXECUTE) {
        flags |= MappingFlags::EXECUTE;
    }
    flags
}

struct KmodMem {
    paddr: PhysAddr,
    vaddr: VirtAddr,
    num_pages: usize,
}

impl SectionMemOps for KmodMem {
    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.vaddr.as_mut_ptr()
    }

    fn as_ptr(&self) -> *const u8 {
        self.vaddr.as_ptr()
    }

    fn change_perms(&mut self, perms: kmod_loader::SectionPerm) -> bool {
        let mapping_flags = section_perms_to_mapping_flags(perms);
        let kspace = kernel_aspace();
        let mut guard = kspace.lock();
        let page_virt = self.vaddr;

        guard
            .protect(page_virt, PAGE_SIZE_4K * self.num_pages, mapping_flags)
            .unwrap();

        true
    }
}

impl Drop for KmodMem {
    fn drop(&mut self) {
        axlog::error!(
            "KmodMem::drop: Deallocating paddr={:?}, num_pages={}",
            self.paddr,
            self.num_pages
        );
        // Deallocate the physical frames
        dealloc_frames(self.paddr, self.num_pages);
    }
}

impl KernelModuleHelper for KmodHelper {
    fn vmalloc(size: usize) -> Box<dyn SectionMemOps> {
        assert!(size % 4096 == 0);

        let num_pages = size / PAGE_SIZE_4K;
        let page_phy = alloc_frames(true, PageSize::Size4K, num_pages, UsageKind::Global).unwrap();
        let virt_start = phys_to_virt(page_phy);

        axlog::error!(
            "KmodHelper::vmalloc: Allocated paddr={:?}, vaddr={:?}, size={}",
            page_phy,
            virt_start,
            size,
        );

        Box::new(KmodMem {
            paddr: page_phy,
            vaddr: virt_start,
            num_pages,
        })
    }

    fn resolve_symbol(name: &str) -> Option<usize> {
        if name.is_empty() {
            axlog::error!("Resolving symbol: {} failed: empty name", name);
            return None;
        }
        let ksym = crate::vfs::KALLSYMS.get()?;
        let res = ksym.lookup_name(name).map(|addr| addr as usize);
        axlog::error!("Resolving symbol: {} => {:x?}", name, res);
        res
    }

    fn flsuh_cache(_addr: usize, _size: usize) {
        flush_tlb(None);
    }
}

// TODO: Handle module
struct ModuleOwnerWrapper(ModuleOwner<KmodHelper>);

unsafe impl Send for ModuleOwnerWrapper {}
unsafe impl Sync for ModuleOwnerWrapper {}

static MODULES: SpinNoPreempt<BTreeMap<String, ModuleOwnerWrapper>> =
    SpinNoPreempt::new(BTreeMap::new());

pub fn init_module(elf: &[u8], params: Option<&str>) -> AxResult<()> {
    let loader = ModuleLoader::<KmodHelper>::new(elf).map_err(|_| AxError::InvalidInput)?;
    let params = if let Some(p) = params {
        CString::new(p).map_err(|_| AxError::InvalidInput)?
    } else {
        CString::new("").unwrap()
    };
    let mut owner = loader
        .load_module(params)
        .map_err(|_| AxError::InvalidInput)?;

    let name = owner.name().unwrap_or("unknown").to_string();

    let res = owner.call_init().expect("Module init can only call once");
    axlog::warn!("Module({}) init returned: {}", name, res);

    let mut modules = MODULES.lock();
    if modules.contains_key(&name) {
        return Err(AxError::AlreadyExists);
    }
    modules.insert(name.to_string(), ModuleOwnerWrapper(owner));
    Ok(())
}

pub fn delete_module(name: &str) -> AxResult<()> {
    let mut modules = MODULES.lock();
    let mut owner_wrapper = modules.remove(name).ok_or(AxError::NotFound)?;

    owner_wrapper.0.call_exit();
    axlog::warn!("Module({}) exited", name);
    Ok(())
}

// For x86_64:
// const MODULE_VADDR_START: usize = 0xffff_ffff_a000_0000;
// const MODULE_VADDR_END: usize = 0xffff_ffff_ff00_0000;

struct StdOut;
impl lwprintf_rs::CustomOutPut for StdOut {
    fn putch(ch: i32) -> i32 {
        ax_print!("{}", ch as u8 as char);
        ch
    }
}

/// Initialize kmod subsystem.
pub fn init_kmod() {
    lwprintf_rs::lwprintf_init::<StdOut>();
    ax_println!("kmod subsystem initialized");
}
