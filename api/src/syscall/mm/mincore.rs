// SPDX-License-Identifier: Apache-2.0
// Copyright (C) 2025 KylinSoft Co., Ltd. <https://www.kylinos.cn/>
// Copyright (C) 2025 Azure-stars <Azure_stars@126.com>
// Copyright (C) 2025 Yuekai Jia <equation618@gmail.com>
// See LICENSES for license details.
//
// This file has been modified by KylinSoft on 2025.

use alloc::vec;

use axerrno::{AxError, AxResult};
use axhal::paging::MappingFlags;
use axtask::current;
use memory_addr::{MemoryAddr, PAGE_SIZE_4K, VirtAddr};
use starry_core::task::AsThread;
use starry_vm::vm_write_slice;

/// Check whether pages are resident in memory.
///
/// The mincore() system call determines whether pages of the calling process's
/// virtual memory are resident in RAM.
///
/// # Arguments
/// * `addr` - Starting address (must be a multiple of the page size)
/// * `length` - Length of the region in bytes (effectively rounded up to next page boundary)
/// * `vec` - Output array containing at least (length+PAGE_SIZE-1)/PAGE_SIZE bytes.
///
/// # Return Value
/// * `Ok(0)` on success
/// * `Err(EAGAIN)` - Kernel is temporarily out of resources (not implemented in StarryOS)
/// * `Err(EFAULT)` - vec points to an invalid address (handled by vm_write_slice)
/// * `Err(EINVAL)` - addr is not a multiple of the page size
/// * `Err(ENOMEM)` - length is greater than (TASK_SIZE - addr), or negative length, or `addr` to `addr`+`length` contained unmapped memory
///
/// # Notes from Linux man page
/// - The least significant bit (bit 0) is set if page is resident in memory
/// - Bits 1-7 are reserved and currently cleared
/// - Information is only a snapshot; pages can be swapped at any moment
///
/// # Linux Errors
/// - EAGAIN:  kernel temporarily out of resources
/// - EFAULT: vec points to invalid address
/// - EINVAL: addr not page-aligned
/// - ENOMEM: length > (TASK_SIZE - addr), negative length, or unmapped memory
pub fn sys_mincore(addr: usize, length: usize, vec: *mut u8) -> AxResult<isize> {
    debug!("sys_mincore <= addr: {addr:#x}, length: {length:#x}, vec: {vec:?}");

    // TODO: Arceos should support unified PAGE_SIZE constant
    let page_size = PAGE_SIZE_4K;

    // EINVAL: addr must be a multiple of the page size
    if !addr.is_multiple_of(page_size) {
        return Err(AxError::InvalidInput);
    }

    // EFAULT: vec must not be null (basic check, vm_write_slice will do full validation)
    if vec.is_null() {
        return Err(AxError::BadAddress);
    }

    // Special case: length=0
    // According to Linux kernel (mm/mincore.c), length=0 returns success
    // WITHOUT validating that addr is mapped.  This is intentional behavior
    // to match POSIX semantics where a zero-length operation is a no-op.
    if length == 0 {
        return Ok(0);
    }

    let start_addr = VirtAddr::from(addr);
    // ENOMEM: Check for overflow (simulates "length > TASK_SIZE - addr")
    // This catches negative lengths interpreted as large unsigned values
    let end_addr = start_addr.checked_add(length).ok_or(AxError::NoMemory)?;

    // Calculate number of pages to check
    let page_count = length.div_ceil(page_size);

    // Get current address space
    let curr = current();
    let aspace = curr.as_thread().proc_data.aspace.lock();

    // Allocate temporary buffer for results
    // Initialize all bytes to 0 (non-resident, all reserved bits clear)
    let mut result_vec = vec![0u8; page_count];

    // Process pages in batches based on query() returned size
    let mut current_page = start_addr.align_down_4k();
    let mut result_index = 0;

    while current_page < end_addr && result_index < page_count {
        // ENOMEM: Check if this page is within a valid VMA
        let area = aspace.find_area(current_page).ok_or(AxError::NoMemory)?;

        // Verify we have at least USER access permission
        if !area.flags().contains(MappingFlags::USER) {
            return Err(AxError::NoMemory);
        }

        // Query page table with batch awareness
        let (is_resident, advance_size) = match aspace.page_table().query(current_page) {
            Ok((_, _, mapped_size)) => {
                // Physical page exists and is resident
                // page_size tells us how many contiguous pages have the same status
                (1u8, mapped_size as _)
            }
            Err(_) => {
                // Page is mapped but not populated (lazy allocation)
                // We need to determine how many contiguous pages are also not populated
                // For safety, we check the next page or use PAGE_SIZE_4K as minimum step
                (0u8, page_size)
            }
        };

        let advance_size = advance_size.max(page_size);

        let remaining_to_check = end_addr.as_usize().saturating_sub(current_page.as_usize());
        let batch_bytes = advance_size.min(remaining_to_check);
        let batch_pages = (batch_bytes / page_size).max(1);

        // Fill in result vector for this batch
        let batch_end = (result_index + batch_pages).min(page_count);
        result_vec[result_index..batch_end].fill(is_resident);

        current_page += advance_size;
        result_index = batch_end;
    }

    // EFAULT: Write result to user space
    // vm_write_slice will return EFAULT if vec is invalid
    vm_write_slice(vec, &result_vec)?;

    Ok(0)
}
