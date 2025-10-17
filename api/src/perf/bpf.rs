use alloc::sync::Arc;
use core::{any::Any, convert::Into, fmt::Debug};

use axerrno::AxResult;
use axhal::paging::PageSize;
use axio::{IoEvents, PollSet, Pollable};
use axmm::backend::SharedPages;
use kbpf_basic::{
    linux_bpf::perf_event_sample_format,
    perf::{PerfProbeArgs, bpf::BpfPerfEvent},
};

use super::PerfEventOps;

pub struct BpfPerfEventWrapper {
    inner: BpfPerfEvent,
    poll_ready: PollSet,
}

impl BpfPerfEventWrapper {
    pub fn new(inner: BpfPerfEvent) -> Self {
        BpfPerfEventWrapper {
            inner,
            poll_ready: PollSet::new(),
        }
    }

    pub fn write_event(&mut self, data: &[u8]) -> AxResult<()> {
        // TODO: remove unwrap
        self.inner.write_event(data).unwrap();
        if self.inner.enabled() {
            self.poll_ready.wake();
        }
        Ok(())
    }
}

impl Debug for BpfPerfEventWrapper {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "BpfPerfEventWrapper")
    }
}

impl PerfEventOps for BpfPerfEventWrapper {
    fn enable(&mut self) -> AxResult<()> {
        self.inner.enable().unwrap();
        Ok(())
    }

    fn disable(&mut self) -> AxResult<()> {
        self.inner.disable().unwrap();
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn custom_mmap(&self) -> bool {
        true
    }

    fn mmap(
        &mut self,
        aspace: &mut axmm::AddrSpace,
        start: memory_addr::VirtAddr,
        length: usize,
        prot: crate::syscall::MmapProt,
        flags: crate::syscall::MmapFlags,
        _offset: usize,
    ) -> AxResult<isize> {
        axlog::warn!(
            "BpfPerfEventWrapper::mmap prot:{:?} flags:{:?}",
            prot,
            flags
        );
        let back = axmm::backend::Backend::new_shared(
            start,
            Arc::new(SharedPages::new(length, PageSize::Size4K)?),
        );
        aspace.map(start, length, prot.into(), true, back)?;

        self.inner.do_mmap(start.as_usize(), length, 0).unwrap();
        Ok(start.as_usize() as isize)
    }
}

impl Pollable for BpfPerfEventWrapper {
    fn poll(&self) -> axio::IoEvents {
        if self.inner.readable() {
            axio::IoEvents::IN
        } else {
            axio::IoEvents::empty()
        }
    }

    fn register(&self, context: &mut core::task::Context<'_>, events: axio::IoEvents) {
        if events.contains(IoEvents::IN) {
            self.poll_ready.register(context.waker());
        }
    }
}

pub fn perf_event_open_bpf(args: PerfProbeArgs) -> BpfPerfEventWrapper {
    // For bpf prog output
    assert_eq!(
        args.sample_type,
        Some(perf_event_sample_format::PERF_SAMPLE_RAW)
    );
    BpfPerfEventWrapper::new(BpfPerfEvent::new(args))
}
