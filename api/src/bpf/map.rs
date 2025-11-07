use alloc::sync::Arc;
use core::ops::{Deref, DerefMut};

use axerrno::{AxError, AxResult};
use axhal::paging::PageSize;
use axio::{PollSet, Pollable};
use kbpf_basic::{
    PollWaker,
    linux_bpf::bpf_attr,
    map::{BpfMapMeta, UnifiedMap},
};
use kspin::{SpinNoPreempt, SpinNoPreemptGuard};
use memory_addr::{PhysAddr, VirtAddr};

use crate::{
    bpf::{
        EbpfKernelAuxiliary,
        tansform::{PerCpuImpl, bpferror_to_axerr},
    },
    file::{FileLike, Kstat, add_file_like},
    syscall::MmapProt,
};

pub struct BpfMap {
    unified_map: SpinNoPreempt<UnifiedMap>,
    poll_ready: Arc<PollSetWrapper>,
}

impl BpfMap {
    pub fn new(unified_map: UnifiedMap, poll_ready: Arc<PollSetWrapper>) -> Self {
        BpfMap {
            unified_map: SpinNoPreempt::new(unified_map),
            poll_ready,
        }
    }

    pub fn unified_map(&self) -> SpinNoPreemptGuard<UnifiedMap> {
        self.unified_map.lock()
    }
}

impl Pollable for BpfMap {
    fn poll(&self) -> axio::IoEvents {
        let map = self.unified_map();

        let mut events = axio::IoEvents::empty();
        if map.map().readable() {
            events |= axio::IoEvents::IN;
        }

        if map.map().writable() {
            events |= axio::IoEvents::OUT;
        }
        events
    }

    fn register(&self, context: &mut core::task::Context<'_>, _events: axio::IoEvents) {
        self.poll_ready.register(context.waker());
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

    fn custom_mmap(&self) -> bool {
        true
    }

    fn mmap(
        &self,
        aspace: &mut axmm::AddrSpace,
        start: VirtAddr,
        length: usize,
        prot: crate::syscall::MmapProt,
        flags: crate::syscall::MmapFlags,
        offset: usize,
    ) -> AxResult<isize> {
        axlog::debug!(
            "BpfMap::mmap() called: start: {:#x}, length: {}, prot: {:?}, flags: {:?}, offset: {}",
            start.as_usize(),
            length,
            prot,
            flags,
            offset
        );
        let phy_addrs = self
            .unified_map()
            .map()
            .map_mmap(
                offset,
                length,
                prot.contains(MmapProt::READ),
                prot.contains(MmapProt::WRITE),
            )
            .unwrap();

        assert_eq!(phy_addrs.len(), length / PageSize::Size4K as usize);

        for (i, phys_addr) in phy_addrs.iter().enumerate() {
            let va = start + i * PageSize::Size4K as usize;
            let pa = PhysAddr::from_usize(*phys_addr);
            aspace.map_linear(va, pa, PageSize::Size4K as _, prot.into())?;
        }

        Ok(start.as_usize() as isize)
    }
}

pub struct PollSetWrapper(PollSet);

impl PollSetWrapper {
    pub fn new() -> Self {
        Self(PollSet::new())
    }
}

impl Deref for PollSetWrapper {
    type Target = PollSet;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for PollSetWrapper {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl PollWaker for PollSetWrapper {
    fn wake_up(&self) {
        self.0.wake();
    }
}

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
