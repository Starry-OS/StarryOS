use axerrno::AxResult;
use axhal::paging::{MappingFlags, PageSize};
use axmm::backend::Backend;
use axtask::current;
use memory_addr::{VirtAddr, align_up_4k};
use starry_core::task::AsThread;

pub fn sys_brk(addr: usize) -> AxResult<isize> {
    let curr = current();
    let proc_data = &curr.as_thread().proc_data;
    let heap_bottom = proc_data.get_heap_bottom() as usize;
    let current_top = proc_data.get_heap_top() as usize;
    let heap_limit = heap_bottom + starry_core::config::USER_HEAP_SIZE_MAX;

    if addr == 0 {
        return Ok(current_top as isize);
    }

    if addr < heap_bottom || addr > heap_limit {
        return Ok(current_top as isize);
    }

    let new_top_aligned = align_up_4k(addr);
    let current_top_aligned = align_up_4k(current_top);
    // Initial heap region end address (already mapped during ELF loading)
    let initial_heap_end = heap_bottom + starry_core::config::USER_HEAP_SIZE;

    // Only map new pages when expanding beyond already mapped region
    // Expansion start should be the greater of initial_heap_end and current_top_aligned
    if new_top_aligned > current_top_aligned {
        let expand_start = VirtAddr::from(initial_heap_end.max(current_top_aligned));
        let expand_size = new_top_aligned.saturating_sub(expand_start.as_usize());

        if expand_size > 0 {
            let mut aspace = proc_data.aspace.lock();
            if aspace
                .map(
                    expand_start,
                    expand_size,
                    MappingFlags::READ | MappingFlags::WRITE | MappingFlags::USER,
                    true,
                    Backend::new_alloc(expand_start, PageSize::Size4K),
                )
                .is_err()
            {
                return Ok(current_top as isize);
            }
            drop(aspace);
        }
    }

    proc_data.set_heap_top(addr);
    Ok(addr as isize)
}
