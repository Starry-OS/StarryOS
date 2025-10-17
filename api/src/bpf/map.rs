use alloc::sync::Arc;

use axerrno::{AxError, AxResult};
use axio::Pollable;
use kbpf_basic::{
    linux_bpf::bpf_attr,
    map::{BpfMapMeta, UnifiedMap},
};
use kspin::{SpinNoPreempt, SpinNoPreemptGuard};

use crate::{
    bpf::tansform::{PerCpuImpl, bpferror_to_axerr},
    file::{FileLike, Kstat, add_file_like},
};
#[derive(Debug)]
pub struct BpfMap {
    unified_map: SpinNoPreempt<UnifiedMap>,
}

impl BpfMap {
    pub fn new(unified_map: UnifiedMap) -> Self {
        BpfMap {
            unified_map: SpinNoPreempt::new(unified_map),
        }
    }

    pub fn unified_map(&self) -> SpinNoPreemptGuard<UnifiedMap> {
        self.unified_map.lock()
    }
}

impl Pollable for BpfMap {
    fn poll(&self) -> axio::IoEvents {
        unimplemented!("BpfMap::poll() is not implemented yet");
    }

    fn register(&self, _context: &mut core::task::Context<'_>, _events: axio::IoEvents) {
        unimplemented!("BpfMap::register() is not implemented yet");
    }
}

impl FileLike for BpfMap {
    fn read(&self, _dst: &mut crate::file::SealedBufMut) -> AxResult<usize> {
        Err(AxError::OperationNotSupported)
    }

    fn write(&self, _src: &mut crate::file::SealedBuf) -> AxResult<usize> {
        Err(AxError::OperationNotSupported)
    }

    fn stat(&self) -> AxResult<crate::file::Kstat> {
        Ok(Kstat::default())
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn core::any::Any + Send + Sync> {
        self
    }

    fn path(&self) -> alloc::borrow::Cow<str> {
        "anon_inode:[bpf_map]".into()
    }
}

pub fn bpf_map_create(attr: &bpf_attr) -> AxResult<isize> {
    let map_meta = BpfMapMeta::try_from(attr).map_err(bpferror_to_axerr)?;
    let unified_map =
        kbpf_basic::map::bpf_map_create::<PerCpuImpl>(map_meta).map_err(bpferror_to_axerr);
    if let Err(e) = &unified_map {
        if e != &AxError::OperationNotSupported {
            axlog::error!("bpf_map_create: failed to create map: {:?}", e);
        }
    }
    let file = Arc::new(BpfMap::new(unified_map?));
    let fd = add_file_like(file, false).map(|fd| fd as _);
    axlog::info!("bpf_map_create: fd: {:?}", fd);
    fd
}
