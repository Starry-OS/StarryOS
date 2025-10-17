pub mod map;
pub mod prog;
pub mod tansform;

use alloc::collections::btree_map::BTreeMap;

use axerrno::{AxError, AxResult};
use kbpf_basic::{
    helper::RawBPFHelperFn,
    linux_bpf::{bpf_attr, bpf_cmd},
    map::{BpfMapGetNextKeyArg, BpfMapUpdateArg},
};
use lazyinit::LazyInit;

use crate::bpf::tansform::{EbpfKernelAuxiliary, bpferror_to_axresult};

pub static BPF_HELPER_FUN_SET: LazyInit<BTreeMap<u32, RawBPFHelperFn>> = LazyInit::new();

pub fn init_bpf() {
    let set = kbpf_basic::helper::init_helper_functions::<EbpfKernelAuxiliary>();
    BPF_HELPER_FUN_SET.init_once(set);
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
