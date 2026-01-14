use alloc::string::String;
use core::{
    ffi::c_char,
    hint::unlikely,
    mem::{MaybeUninit, transmute},
};

use axerrno::{AxError, AxResult};
use axhal::{
    paging::MappingFlags,
    trap::{PAGE_FAULT, register_trap_handler},
};
use axio::prelude::*;
use axtask::current;
use memory_addr::VirtAddr;
use starry_core::task::AsThread;
use starry_vm::{vm_load_until_nul, vm_read_slice, vm_write_slice};

#[register_trap_handler(PAGE_FAULT)]
fn handle_page_fault(vaddr: VirtAddr, access_flags: MappingFlags) -> bool {
    debug!("Page fault at {vaddr:#x}, access_flags: {access_flags:#x?}");

    let curr = current();
    let Some(thr) = curr.try_as_thread() else {
        return false;
    };

    if unlikely(!thr.is_accessing_user_memory()) {
        return false;
    }

    thr.proc_data
        .aspace
        .lock()
        .handle_page_fault(vaddr, access_flags)
}

pub fn vm_load_string(ptr: *const c_char) -> AxResult<String> {
    #[allow(clippy::unnecessary_cast)]
    let bytes = vm_load_until_nul(ptr as *const u8)?;
    String::from_utf8(bytes).map_err(|_| AxError::IllegalBytes)
}

/// A read-only buffer in the VM's memory.
///
/// It implements the `axio::Read` trait, allowing it to be used with other I/O
/// operations.
pub struct VmBytes {
    /// The pointer to the start of the buffer in the VM's memory.
    pub ptr: *const u8,
    /// The length of the buffer.
    pub len: usize,
}

impl VmBytes {
    /// Creates a new `VmBytes` from a raw pointer and a length.
    pub fn new(ptr: *const u8, len: usize) -> Self {
        Self { ptr, len }
    }

    /// Casts the `VmBytes` to a mutable `VmBytesMut`.
    pub fn cast_mut(&self) -> VmBytesMut {
        VmBytesMut::new(self.ptr as *mut u8, self.len)
    }
}

impl Read for VmBytes {
    /// Reads bytes from the VM's memory into the provided buffer.
    fn read(&mut self, buf: &mut [u8]) -> axio::Result<usize> {
        let len = self.len.min(buf.len());
        vm_read_slice(self.ptr, unsafe {
            transmute::<&mut [u8], &mut [MaybeUninit<u8>]>(&mut buf[..len])
        })?;
        self.ptr = self.ptr.wrapping_add(len);
        self.len -= len;
        Ok(len)
    }
}

impl IoBuf for VmBytes {
    fn remaining(&self) -> usize {
        self.len
    }
}

/// A mutable buffer in the VM's memory.
///
/// It implements the `axio::Write` trait, allowing it to be used with other I/O
/// operations.
pub struct VmBytesMut {
    /// The pointer to the start of the buffer in the VM's memory.
    pub ptr: *mut u8,
    /// The length of the buffer.
    pub len: usize,
}

impl VmBytesMut {
    /// Creates a new `VmBytesMut` from a raw pointer and a length.
    pub fn new(ptr: *mut u8, len: usize) -> Self {
        Self { ptr, len }
    }

    /// Casts the `VmBytesMut` to a read-only `VmBytes`.
    pub fn cast_const(&self) -> VmBytes {
        VmBytes::new(self.ptr, self.len)
    }
}

impl Write for VmBytesMut {
    /// Writes bytes from the provided buffer into the VM's memory.
    fn write(&mut self, buf: &[u8]) -> axio::Result<usize> {
        let len = self.len.min(buf.len());
        vm_write_slice(self.ptr, &buf[..len])?;
        self.ptr = self.ptr.wrapping_add(len);
        self.len -= len;
        Ok(len)
    }

    /// Flushes the buffer. This is a no-op for `VmBytesMut`.
    fn flush(&mut self) -> axio::Result {
        Ok(())
    }
}

impl IoBufMut for VmBytesMut {
    fn remaining_mut(&self) -> usize {
        self.len
    }
}
