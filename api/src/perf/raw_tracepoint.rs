use alloc::{boxed::Box, sync::Arc};
use core::sync::atomic::AtomicUsize;

use axerrno::{AxError, AxResult};
use axio::Pollable;
use kbpf_basic::raw_tracepoint::BpfRawTracePointArg;
use tracepoint::TracePoint;

use crate::{
    file::{FileLike, add_file_like, get_file_like},
    lock_api::KSpinNoPreempt,
    perf::tracepoint::TracePointPerfCallBack,
    tracepoint::KernelTraceAux,
};

pub struct RawTracepointPerfEvent {
    tp: &'static TracePoint<KSpinNoPreempt<()>, KernelTraceAux>,
    callback_id: usize,
}

impl Pollable for RawTracepointPerfEvent {
    fn poll(&self) -> axio::IoEvents {
        panic!("RawTracepointPerfEvent::poll() should not be called");
    }

    fn register(&self, _context: &mut core::task::Context<'_>, _events: axio::IoEvents) {
        panic!("RawTracepointPerfEvent::register() should not be called");
    }
}

impl FileLike for RawTracepointPerfEvent {
    fn read(&self, _dst: &mut crate::file::SealedBufMut) -> AxResult<usize> {
        Err(AxError::OperationNotSupported)
    }

    fn write(&self, _src: &mut crate::file::SealedBuf) -> AxResult<usize> {
        Err(AxError::OperationNotSupported)
    }

    fn stat(&self) -> AxResult<crate::file::Kstat> {
        Ok(crate::file::Kstat::default())
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn core::any::Any + Send + Sync> {
        self
    }

    fn path(&self) -> alloc::borrow::Cow<str> {
        "anon_inode:[raw_tracepoint_perf_event]".into()
    }
}

impl RawTracepointPerfEvent {
    pub fn new(
        tp: &'static TracePoint<KSpinNoPreempt<()>, KernelTraceAux>,
        bpf_prog: Arc<dyn FileLike>,
    ) -> AxResult<Self> {
        static CALLBACK_ID: AtomicUsize = AtomicUsize::new(0);

        let vm = super::bpf::create_basic_ebpf_vm(bpf_prog.clone())?;

        let callback = Box::new(TracePointPerfCallBack::new(bpf_prog, vm));

        let id = CALLBACK_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        tp.register_raw_event_callback(id, callback);

        axlog::warn!(
            "Registered BPF program for tracepoint: {}:{} with ID: {}",
            tp.system(),
            tp.name(),
            id
        );
        Ok(RawTracepointPerfEvent {
            tp,
            callback_id: id,
        })
    }
}

impl Drop for RawTracepointPerfEvent {
    fn drop(&mut self) {
        axlog::warn!(
            "Unregistering BPF program for rawtracepoint: {}:{} with ID: {}",
            self.tp.system(),
            self.tp.name(),
            self.callback_id
        );
        self.tp.unregister_raw_event_callback(self.callback_id);
    }
}

pub fn bpf_raw_tracepoint_open(arg: BpfRawTracePointArg) -> AxResult<isize> {
    let tp_manager = crate::tracepoint::tracepoint_manager();
    let tp_map = tp_manager.tracepoint_map();

    let mut tp = None;
    for t in tp_map.values() {
        if t.name() == arg.name {
            // found the tracepoint
            tp = Some(*t);
            break;
        }
    }
    let tp = match tp {
        Some(tp) => tp,
        None => return Err(AxError::InvalidInput),
    };

    let prog = get_file_like(arg.prog_fd as _)?;
    let raw_tp = RawTracepointPerfEvent::new(tp, prog)?;
    let raw_tp = Arc::new(raw_tp);
    let fd = add_file_like(raw_tp, false).map(|fd| fd as _)?;

    Ok(fd)
}
