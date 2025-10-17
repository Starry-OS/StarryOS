use alloc::vec;

use axerrno::AxResult;
use axio::Read;
use kbpf_basic::linux_bpf::perf_event_attr;
use starry_vm::VmBytes;

use crate::perf::perf_event_open;

pub fn sys_perf_event_open(
    attr: *const u8,
    pid: i32,
    cpu: i32,
    group_fd: i32,
    flags: u32,
) -> AxResult<isize> {
    let mut buf = VmBytes::new(attr as *mut u8, size_of::<perf_event_attr>());

    let mut attr = vec![0u8; core::mem::size_of::<perf_event_attr>()];
    buf.read(&mut attr)?;
    let attr = unsafe { &*(attr.as_ptr() as *const perf_event_attr) };

    perf_event_open(attr, pid, cpu, group_fd, flags)
}
