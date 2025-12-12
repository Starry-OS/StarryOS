//! vDSO data management.

extern crate alloc;
extern crate axlog;
use alloc::vec::Vec;

use axerrno::{AxError, AxResult};
use axhal::paging::MappingFlags;
use axmm::AddrSpace;
use kernel_elf_parser::{AuxEntry, AuxType};
use memory_addr::{MemoryAddr, PAGE_SIZE_4K};
use starry_vdso::{
    guard::VdsoAllocGuard,
    vdso::{prepare_vdso_pages, vdso_data_paddr},
};

/// Load vDSO into the given user address space and update auxv accordingly.
pub fn load_vdso_data(auxv: &mut Vec<AuxEntry>, uspace: &mut AddrSpace) -> AxResult<()> {
    unsafe extern "C" {
        static vdso_start: u8;
        static vdso_end: u8;
    }
    let (vdso_kstart, vdso_kend) = unsafe {
        (
            &vdso_start as *const u8 as usize,
            &vdso_end as *const u8 as usize,
        )
    };
    info!("vdso_kstart: {vdso_kstart:#x}, vdso_kend: {vdso_kend:#x}");

    if vdso_kend <= vdso_kstart {
        warn!(
            "vDSO binary is missing or invalid: vdso_kstart={vdso_kstart:#x}, \
             vdso_kend={vdso_kend:#x}. vDSO will not be loaded and AT_SYSINFO_EHDR will not be \
             set."
        );
        return Err(AxError::InvalidExecutable);
    }

    let (vdso_paddr_page, vdso_bytes, vdso_size, vdso_page_offset, alloc_info) =
        prepare_vdso_pages(vdso_kstart, vdso_kend).map_err(|_| AxError::InvalidExecutable)?;

    let mut alloc_guard = VdsoAllocGuard::new(alloc_info);

    let (_base_addr, vdso_user_addr) =
        starry_vdso::vdso::calculate_vdso_aslr_addr(vdso_kstart, vdso_kend, vdso_page_offset);

    match kernel_elf_parser::ELFHeadersBuilder::new(vdso_bytes).and_then(|b| {
        let range = b.ph_range();
        b.build(&vdso_bytes[range.start as usize..range.end as usize])
    }) {
        Ok(headers) => {
            map_vdso_segments(
                headers,
                vdso_user_addr,
                vdso_paddr_page,
                vdso_page_offset,
                uspace,
            )?;
            alloc_guard.disarm();
        }
        Err(_) => {
            info!("vDSO ELF parsing failed, using fallback mapping");
            let map_user_start = if vdso_page_offset == 0 {
                vdso_user_addr
            } else {
                vdso_user_addr - vdso_page_offset
            };
            uspace
                .map_linear(
                    map_user_start.into(),
                    vdso_paddr_page,
                    vdso_size,
                    MappingFlags::READ | MappingFlags::EXECUTE | MappingFlags::USER,
                )
                .map_err(|_| AxError::InvalidExecutable)?;
            alloc_guard.disarm();
        }
    }

    map_vvar_and_push_aux(auxv, vdso_user_addr, uspace)?;

    Ok(())
}

fn map_vvar_and_push_aux(
    auxv: &mut Vec<AuxEntry>,
    vdso_user_addr: usize,
    uspace: &mut AddrSpace,
) -> AxResult<()> {
    use starry_vdso::config::VVAR_PAGES;
    let vvar_user_addr = vdso_user_addr - VVAR_PAGES * PAGE_SIZE_4K;
    let vvar_paddr = vdso_data_paddr();

    uspace
        .map_linear(
            vvar_user_addr.into(),
            vvar_paddr.into(),
            VVAR_PAGES * PAGE_SIZE_4K,
            MappingFlags::READ | MappingFlags::USER,
        )
        .map_err(|_| AxError::InvalidExecutable)?;

    info!(
        "Mapped vvar pages at user {:#x}..{:#x} -> paddr {:#x}",
        vvar_user_addr,
        vvar_user_addr + VVAR_PAGES * PAGE_SIZE_4K,
        vvar_paddr,
    );

    let aux_entry = AuxEntry::new(AuxType::SYSINFO_EHDR, vdso_user_addr);
    auxv.push(aux_entry);

    Ok(())
}

fn map_vdso_segments(
    headers: kernel_elf_parser::ELFHeaders,
    vdso_user_addr: usize,
    vdso_paddr_page: axhal::mem::PhysAddr,
    vdso_page_offset: usize,
    uspace: &mut AddrSpace,
) -> AxResult<()> {
    info!("vDSO ELF parsed successfully, mapping segments");
    for ph in headers
        .ph
        .iter()
        .filter(|ph| ph.get_type() == Ok(xmas_elf::program::Type::Load))
    {
        let vaddr = ph.virtual_addr as usize;
        let seg_pad = vaddr.align_offset_4k() + vdso_page_offset;
        let seg_align_size =
            (ph.mem_size as usize + seg_pad + PAGE_SIZE_4K - 1) & !(PAGE_SIZE_4K - 1);

        let map_base_user = vdso_user_addr & !(PAGE_SIZE_4K - 1);
        let seg_user_start = map_base_user + vaddr.align_down_4k();
        let seg_paddr = vdso_paddr_page + vaddr.align_down_4k();

        let mut flags = MappingFlags::USER;
        if ph.flags.is_read() {
            flags |= MappingFlags::READ;
        }
        if ph.flags.is_write() {
            flags |= MappingFlags::WRITE;
        }
        if ph.flags.is_execute() {
            flags |= MappingFlags::EXECUTE;
        }
        uspace
            .map_linear(seg_user_start.into(), seg_paddr, seg_align_size, flags)
            .map_err(|_| AxError::InvalidExecutable)?;
    }
    Ok(())
}
