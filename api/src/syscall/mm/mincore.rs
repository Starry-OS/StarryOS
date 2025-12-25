// SPDX-License-Identifier: Apache-2.0
// Copyright (C) 2025 KylinSoft Co., Ltd. <https://www.kylinos.cn/>
// Copyright (C) 2025 Azure-stars <Azure_stars@126.com>
// Copyright (C) 2025 Yuekai Jia <equation618@gmail.com>
// See LICENSES for license details.
//
// This file has been modified by KylinSoft on 2025.

use alloc::vec;
use axerrno::{AxError, AxResult, LinuxError};
use axtask::current;
use axhal::paging::MappingFlags;
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

    // EINVAL: addr must be a multiple of the page size
    // TODO: Arceos should support unified PAGE_SIZE constant
    if !addr.is_multiple_of(PAGE_SIZE_4K) {
        return Err(AxError::InvalidInput);
    }

    // EFAULT: vec must not be null (basic check, vm_write_slice will do full validation)
    if vec.is_null() {
        return Err(AxError::BadAddress);
    }

    // Special case: length 0 is valid and returns immediately
    if length == 0 {
        return Ok(0);
    }

    let start_addr = VirtAddr::from(addr);

    // ENOMEM: Check for overflow (simulates "length > TASK_SIZE - addr")
    // This catches negative lengths interpreted as large unsigned values
    start_addr
        .checked_add(length)
        .ok_or({LinuxError::ENOMEM
    })?;

    // Calculate number of pages to check
    // Formula from man page: (length + PAGE_SIZE - 1) / PAGE_SIZE
    let page_count = length.div_ceil(PAGE_SIZE_4K);

    // Get current address space
    let curr = current();
    let aspace = curr.as_thread().proc_data.aspace.lock();

    // Allocate temporary buffer for results
    // Initialize all bytes to 0 (non-resident, all reserved bits clear)
    let mut result_vec = vec![0u8; page_count];

    // Check each page in the range [addr, addr+length)
    let mut current_page = start_addr.align_down_4k();

    for res in result_vec.iter_mut().take(page_count) {
        // ENOMEM: Check if this page is within a valid VMA (Virtual Memory Area)
        // Linux returns ENOMEM for "unmapped memory"
        aspace.find_area(current_page).ok_or({
            // This address is not mapped - return ENOMEM per Linux spec
            LinuxError::ENOMEM
        })?;

        // Verify we have at least USER access permission
        // (Though find_area likely already ensures this for user mappings)
        if !aspace.can_access_range(current_page, PAGE_SIZE_4K, MappingFlags::USER) {
            // Mapped but not accessible - treat as ENOMEM per Linux behavior
            return Err(LinuxError::ENOMEM.into());
        }

        // Query the page table to check physical page presence
        // In StarryOS with lazy allocation:
        // - query() succeeds -> physical page is allocated and resident (return 1)
        // - query() fails -> page mapped but not populated yet (return 0)
        let is_resident = match aspace.page_table().query(current_page) {
            Ok((..)) => 1u8,
            Err(_) => 0u8,
        };
        *res = is_resident;
        current_page += PAGE_SIZE_4K;
    }

    // EFAULT: Write result to user space
    // vm_write_slice will return EFAULT if vec is invalid
    vm_write_slice(vec, &result_vec)?;

    Ok(0)
}
