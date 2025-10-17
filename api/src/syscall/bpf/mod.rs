use alloc::vec;

use axerrno::{AxError, AxResult};
use axio::Read;
use kbpf_basic::linux_bpf::{bpf_attr, bpf_cmd};
use starry_vm::VmBytes;

/// Handle the bpf syscall
pub fn sys_bpf(cmd: u32, attr: *mut u8, size: u32) -> AxResult<isize> {
    let mut buf = vec![0u8; size as usize];
    let _l = VmBytes::new(attr, size as _).read(&mut buf)?;

    let attr = unsafe { &*(buf.as_ptr() as *const bpf_attr) };
    let cmd = bpf_cmd::try_from(cmd).map_err(|_| AxError::InvalidInput)?;
    crate::bpf::bpf(cmd, &attr)
}
