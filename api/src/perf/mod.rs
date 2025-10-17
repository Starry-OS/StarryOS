mod bpf;
mod kprobe;
mod tracepoint;

use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
};
use core::{any::Any, ffi::c_void, fmt::Debug};

use axerrno::{AxError, AxResult};
use axio::Pollable;
use hashbrown::HashMap;
use kbpf_basic::{
    linux_bpf::{perf_event_attr, perf_type_id},
    perf::{PerfEventIoc, PerfProbeArgs},
};
use kspin::{SpinNoPreempt, SpinNoPreemptGuard};
use lazyinit::LazyInit;

use crate::{
    bpf::tansform::EbpfKernelAuxiliary,
    file::{FileLike, Kstat, add_file_like, get_file_like},
    perf::bpf::BpfPerfEventWrapper,
};

pub trait PerfEventOps: Pollable + Send + Sync + Debug {
    fn enable(&mut self) -> AxResult<()>;
    fn disable(&mut self) -> AxResult<()>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn custom_mmap(&self) -> bool {
        false
    }
    fn set_bpf_prog(&mut self, _bpf_prog: Arc<dyn FileLike>) -> AxResult<()> {
        Err(AxError::OperationNotSupported)
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
                event.set_bpf_prog(file)?;
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
    axlog::info!("perf_event_process: {:#?}", args);
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
        perf_type_id::PERF_TYPE_TRACEPOINT => {
            let tracepoint_event = tracepoint::perf_event_open_tracepoint(args)?;
            Box::new(tracepoint_event)
        }
        _ => {
            unimplemented!("perf_event_process: unknown type: {:?}", args);
        }
    };
    let event = Arc::new(PerfEvent::new(event)) as Arc<dyn FileLike>;
    let fd = add_file_like(event.clone(), false).map(|fd| fd as _)?;

    PERF_FILE
        .get()
        .unwrap()
        .lock()
        .insert(fd, Arc::downgrade(&event));

    axlog::info!("perf_event_open: fd: {:?}", fd);
    Ok(fd as _)
}

static PERF_FILE: LazyInit<SpinNoPreempt<HashMap<usize, Weak<dyn FileLike>>>> = LazyInit::new();

pub fn perf_event_init() {
    PERF_FILE.init_once(SpinNoPreempt::new(HashMap::new()));
}

pub fn perf_event_output(_ctx: *mut c_void, fd: usize, _flags: u32, data: &[u8]) -> AxResult<()> {
    let mut perf_file_map = PERF_FILE.get().unwrap().lock();
    let perf_file_weak = perf_file_map.get(&fd).ok_or(AxError::NotFound)?;

    let Some(file) = perf_file_weak.upgrade() else {
        // The file has been dropped, remove the weak reference from the map
        perf_file_map.remove(&fd);
        return Err(AxError::NotFound);
    };

    let bpf_event_file = file.into_any().downcast::<PerfEvent>().unwrap();
    let mut event = bpf_event_file.event();
    let event = event
        .as_any_mut()
        .downcast_mut::<BpfPerfEventWrapper>()
        .unwrap();
    event.write_event(data).unwrap();
    Ok(())
}
