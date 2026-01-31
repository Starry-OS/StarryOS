#![no_std]
extern crate alloc;

use axcpu::uspace::UserContext;
use kmod::{exit_fn, init_fn, module};

pub mod map;
pub mod prog;
pub mod tansform;

use alloc::vec;

use axerrno::{AxError, AxResult};
use axio::Read;
use kbpf_basic::{
    linux_bpf::{bpf_attr, bpf_cmd},
    map::{BpfMapGetNextKeyArg, BpfMapUpdateArg},
    raw_tracepoint::BpfRawTracePointArg,
};
use starry_api::{bpf::tansform::EbpfKernelAuxiliary, mm::VmBytes, syscall::SyscallHandler};
use syscalls::Sysno;

use crate::tansform::bpferror_to_axresult;

/// Handle the bpf syscall
pub fn sys_bpf(cmd: u32, attr: *mut u8, size: u32) -> AxResult<isize> {
    let mut buf = vec![0u8; size as usize];
    let _l = VmBytes::new(attr, size as _).read(&mut buf)?;

    let attr = unsafe { &*(buf.as_ptr() as *const bpf_attr) };
    let cmd = bpf_cmd::try_from(cmd).map_err(|_| AxError::InvalidInput)?;
    bpf(cmd, &attr)
}

pub fn bpf(cmd: bpf_cmd, attr: &bpf_attr) -> AxResult<isize> {
    let update_arg = BpfMapUpdateArg::from(attr);
    match cmd {
        // Map related commands
        bpf_cmd::BPF_MAP_CREATE => map::bpf_map_create(attr),
        bpf_cmd::BPF_MAP_UPDATE_ELEM => {
            kbpf_basic::map::bpf_map_update_elem::<EbpfKernelAuxiliary>(update_arg)
                .map_or_else(bpferror_to_axresult, |_| Ok(0))
        }
        bpf_cmd::BPF_MAP_LOOKUP_ELEM => {
            kbpf_basic::map::bpf_lookup_elem::<EbpfKernelAuxiliary>(update_arg)
                .map_or_else(bpferror_to_axresult, |_| Ok(0))
        }
        bpf_cmd::BPF_MAP_GET_NEXT_KEY => {
            let update_arg = BpfMapGetNextKeyArg::from(attr);
            kbpf_basic::map::bpf_map_get_next_key::<EbpfKernelAuxiliary>(update_arg)
                .map_or_else(bpferror_to_axresult, |_| Ok(0))
        }
        bpf_cmd::BPF_MAP_DELETE_ELEM => {
            kbpf_basic::map::bpf_map_delete_elem::<EbpfKernelAuxiliary>(update_arg)
                .map_or_else(bpferror_to_axresult, |_| Ok(0))
        }
        bpf_cmd::BPF_MAP_LOOKUP_AND_DELETE_ELEM => {
            kbpf_basic::map::bpf_map_lookup_and_delete_elem::<EbpfKernelAuxiliary>(update_arg)
                .map_or_else(bpferror_to_axresult, |_| Ok(0))
        }
        bpf_cmd::BPF_MAP_LOOKUP_BATCH => {
            kbpf_basic::map::bpf_map_lookup_batch::<EbpfKernelAuxiliary>(update_arg)
                .map_or_else(bpferror_to_axresult, |_| Ok(0))
        }
        bpf_cmd::BPF_MAP_FREEZE => {
            kbpf_basic::map::bpf_map_freeze::<EbpfKernelAuxiliary>(update_arg.map_fd)
                .map_or_else(bpferror_to_axresult, |_| Ok(0))
        }
        // Attaches the program to the given tracepoint.
        bpf_cmd::BPF_RAW_TRACEPOINT_OPEN => {
            let arg = BpfRawTracePointArg::try_from_bpf_attr::<EbpfKernelAuxiliary>(attr)
                .map_err(|_| AxError::InvalidInput)?;
            starry_api::perf::raw_tracepoint::bpf_raw_tracepoint_open(arg)
        }
        // Program related commands
        bpf_cmd::BPF_PROG_LOAD => prog::bpf_prog_load(attr),
        // Object creation commands
        bpf_cmd::BPF_BTF_LOAD | bpf_cmd::BPF_LINK_CREATE | bpf_cmd::BPF_OBJ_GET_INFO_BY_FD => {
            axlog::warn!("bpf cmd: [{:?}] not implemented", cmd);
            Err(AxError::OperationNotSupported)
        }
        ty => {
            unimplemented!("bpf cmd: [{:?}] not implemented", ty)
        }
    }
}

struct Handler;

impl SyscallHandler for Handler {
    fn handle(&self, uctx: &mut UserContext) -> Result<isize, AxError> {
        sys_bpf(uctx.arg0() as _, uctx.arg1() as _, uctx.arg2() as _)
    }
}

#[init_fn]
pub fn kebpf_init() -> i32 {
    axlog::ax_println!("Hello, eBPF Kernel Module!");
    let res = starry_api::syscall::register_syscall_handler(Sysno::bpf, &Handler);
    if let Err(e) = res {
        axlog::error!("Failed to register bpf syscall handler: {:?}", e);
        return -1;
    }
    0
}

#[exit_fn]
fn kebpf_exit() {
    axlog::ax_println!("Goodbye, eBPF Kernel Module!");
}

module!(
    name: "kebpf",
    license: "GPL",
    description: "kernel eBPF module",
    version: "0.1.0",
);
