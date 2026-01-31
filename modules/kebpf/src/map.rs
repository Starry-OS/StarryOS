use alloc::sync::Arc;

use axerrno::{AxError, AxResult};
use kbpf_basic::{linux_bpf::bpf_attr, map::BpfMapMeta};
use starry_api::{
    bpf::{
        map::{BpfMap, PollSetWrapper},
        tansform::{EbpfKernelAuxiliary, PerCpuImpl},
    },
    file::add_file_like,
};

use crate::tansform::bpferror_to_axerr;

pub fn bpf_map_create(attr: &bpf_attr) -> AxResult<isize> {
    let map_meta = BpfMapMeta::try_from(attr).map_err(bpferror_to_axerr)?;
    axlog::debug!("The map attr is {:#?}", map_meta);

    let poll_ready = Arc::new(PollSetWrapper::new());

    let unified_map = kbpf_basic::map::bpf_map_create::<EbpfKernelAuxiliary, PerCpuImpl>(
        map_meta,
        Some(poll_ready.clone()),
    )
    .map_err(bpferror_to_axerr);

    if let Err(e) = &unified_map {
        if e != &AxError::OperationNotSupported {
            axlog::error!("bpf_map_create: failed to create map: {:?}", e);
        }
    }

    let file = Arc::new(BpfMap::new(unified_map?, poll_ready));
    let fd = add_file_like(file, false).map(|fd| fd as _);
    axlog::info!("bpf_map_create: fd: {:?}", fd);
    fd
}
