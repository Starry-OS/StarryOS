mod bpf;
mod kprobe;

use alloc::{boxed::Box, sync::Arc};
use core::{any::Any, ffi::c_void, fmt::Debug};

use axerrno::{AxError, AxResult};
use axio::Pollable;
use kbpf_basic::{
    linux_bpf::{perf_event_attr, perf_type_id},
    perf::{PerfEventIoc, PerfProbeArgs},
};
use kspin::{SpinNoPreempt, SpinNoPreemptGuard};

use crate::{
    bpf::tansform::EbpfKernelAuxiliary,
    file::{FileLike, Kstat, add_file_like, get_file_like},
    perf::{bpf::BpfPerfEventWrapper, kprobe::KprobePerfEvent},
};

pub trait PerfEventOps: Pollable + Send + Sync + Debug {
    fn enable(&mut self) -> AxResult<()>;
    fn disable(&mut self) -> AxResult<()>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn custom_mmap(&self) -> bool {
        false
    }

    fn mmap(
        &mut self,
        _aspace: &mut axmm::AddrSpace,
        _start: memory_addr::VirtAddr,
        _length: usize,
        _prot: crate::syscall::MmapProt,
        _flags: crate::syscall::MmapFlags,
        _offset: usize,
    ) -> AxResult<isize> {
        Err(AxError::OperationNotSupported)
    }
}

#[derive(Debug)]
pub struct PerfEvent {
    event: SpinNoPreempt<Box<dyn PerfEventOps>>,
}

impl PerfEvent {
    pub fn new(event: Box<dyn PerfEventOps>) -> Self {
        PerfEvent {
            event: SpinNoPreempt::new(event),
        }
    }

    pub fn event(&self) -> SpinNoPreemptGuard<Box<dyn PerfEventOps>> {
        self.event.lock()
    }
}

impl Pollable for PerfEvent {
    fn poll(&self) -> axio::IoEvents {
        self.event.lock().poll()
    }

    fn register(&self, context: &mut core::task::Context<'_>, events: axio::IoEvents) {
        self.event.lock().register(context, events)
    }
}

impl FileLike for PerfEvent {
    fn read(&self, _dst: &mut crate::file::SealedBufMut) -> AxResult<usize> {
        todo!()
    }

    fn write(&self, _src: &mut crate::file::SealedBuf) -> AxResult<usize> {
        todo!()
    }

    fn stat(&self) -> AxResult<crate::file::Kstat> {
        Ok(Kstat::default())
    }

    fn into_any(self: ringbuf::Arc<Self>) -> ringbuf::Arc<dyn Any + Send + Sync> {
        self
    }

    fn path(&self) -> alloc::borrow::Cow<str> {
        "anon_inode:[perf_event]".into()
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> AxResult<usize> {
        let req = PerfEventIoc::try_from(cmd).map_err(|_| AxError::InvalidInput)?;
        axlog::info!("perf_event_ioctl: request: {:?}, arg: {}", req, arg);
        match req {
            PerfEventIoc::Enable => {
                self.event.lock().enable().unwrap();
            }
            PerfEventIoc::Disable => {
                self.event.lock().disable().unwrap();
            }
            PerfEventIoc::SetBpf => {
                axlog::warn!("perf_event_ioctl: PERF_EVENT_IOC_SET_BPF, arg: {}", arg);
                let bpf_prog_fd = arg;
                let file = get_file_like(bpf_prog_fd as _)?;

                let mut event = self.event.lock();
                let kprobe_event = event
                    .as_any_mut()
                    .downcast_mut::<KprobePerfEvent>()
                    .ok_or(AxError::InvalidInput)?;
                kprobe_event.set_bpf_prog(file)?;
            }
        }
        Ok(0)
    }

    fn custom_mmap(&self) -> bool {
        self.event.lock().custom_mmap()
    }

    fn mmap(
        &self,
        aspace: &mut axmm::AddrSpace,
        addr: memory_addr::VirtAddr,
        length: usize,
        prot: crate::syscall::MmapProt,
        flags: crate::syscall::MmapFlags,
        offset: usize,
    ) -> AxResult<isize> {
        self.event
            .lock()
            .mmap(aspace, addr, length, prot, flags, offset)
    }
}

pub fn perf_event_open(
    attr: &perf_event_attr,
    pid: i32,
    cpu: i32,
    group_fd: i32,
    flags: u32,
) -> AxResult<isize> {
    let args =
        PerfProbeArgs::try_from_perf_attr::<EbpfKernelAuxiliary>(attr, pid, cpu, group_fd, flags)
            .unwrap();
    axlog::warn!("perf_event_process: {:#?}", args);
    let event: Box<dyn PerfEventOps> = match args.type_ {
        // Kprobe
        // See /sys/bus/event_source/devices/kprobe/type
        perf_type_id::PERF_TYPE_MAX => {
            let kprobe_event = kprobe::perf_event_open_kprobe(args);
            Box::new(kprobe_event)
        }
        perf_type_id::PERF_TYPE_SOFTWARE => {
            let bpf_event = bpf::perf_event_open_bpf(args);
            Box::new(bpf_event)
        }
        _ => {
            unimplemented!("perf_event_process: unknown type: {:?}", args);
        }
    };
    let event = Arc::new(PerfEvent::new(event));
    let fd = add_file_like(event, false).map(|fd| fd as _);
    fd
}

pub fn perf_event_output(_ctx: *mut c_void, fd: usize, _flags: u32, data: &[u8]) -> AxResult<()> {
    // axlog::error!("perf_event_output: fd: {}, data: {:?}", fd, data.len());
    let file = get_file_like(fd as _)?;
    let bpf_event_file = file.into_any().downcast::<PerfEvent>().unwrap();
    let mut event = bpf_event_file.event();
    let event = event.as_any_mut().downcast_mut::<BpfPerfEventWrapper>().unwrap();
    event.write_event(data).unwrap();
    Ok(())
}
