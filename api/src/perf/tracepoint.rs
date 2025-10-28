use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::sync::atomic::AtomicUsize;

use axerrno::{AxError, AxResult};
use axio::Pollable;
use kbpf_basic::perf::{PerfProbeArgs, PerfProbeConfig};
use kspin::SpinNoPreempt;
use rbpf::EbpfVmRaw;
use tracepoint::{RawTracePointCallBackFunc, TracePoint, TracePointCallBackFunc};

use crate::{
    file::FileLike, lock_api::KSpinNoPreempt, perf::PerfEventOps, tracepoint::KernelTraceAux,
};

#[derive(Debug)]
pub struct TracepointPerfEvent {
    _args: PerfProbeArgs,
    tp: &'static TracePoint<KSpinNoPreempt<()>, KernelTraceAux>,
    ebpf_list: SpinNoPreempt<Vec<usize>>,
}

impl TracepointPerfEvent {
    pub fn new(
        args: PerfProbeArgs,
        tp: &'static TracePoint<KSpinNoPreempt<()>, KernelTraceAux>,
    ) -> TracepointPerfEvent {
        TracepointPerfEvent {
            _args: args,
            tp,
            ebpf_list: SpinNoPreempt::new(Vec::new()),
        }
    }
}

pub struct TracePointPerfCallBack {
    _bpf_prog_file: Arc<dyn FileLike>,
    vm: EbpfVmRaw<'static>,
}

impl TracePointPerfCallBack {
    pub fn new(bpf_prog_file: Arc<dyn FileLike>, vm: EbpfVmRaw<'static>) -> Self {
        TracePointPerfCallBack {
            _bpf_prog_file: bpf_prog_file,
            vm,
        }
    }
}

unsafe impl Send for TracePointPerfCallBack {}
unsafe impl Sync for TracePointPerfCallBack {}
// pub struct TracePointPerfCallBack(BasicPerfEbpfCallBack);

impl TracePointCallBackFunc for TracePointPerfCallBack {
    fn call(&self, entry: &[u8]) {
        // ebpf needs a mutable slice
        let entry =
            unsafe { core::slice::from_raw_parts_mut(entry.as_ptr() as *mut u8, entry.len()) };
        let res = self.vm.execute_program(entry);
        if res.is_err() {
            axlog::error!("kprobe callback error: {:?}", res);
        }
    }
}

impl RawTracePointCallBackFunc for TracePointPerfCallBack {
    fn call(&self, args: &[u64]) {
        let args =
            unsafe { core::slice::from_raw_parts_mut(args.as_ptr() as *mut u8, args.len() * 8) };
        let res = self.vm.execute_program(args);
        if res.is_err() {
            axlog::error!("raw tracepoint callback error: {:?}", res);
        }
    }
}

impl Pollable for TracepointPerfEvent {
    fn poll(&self) -> axio::IoEvents {
        panic!("TracepointPerfEvent::poll() should not be called");
    }

    fn register(&self, _context: &mut core::task::Context<'_>, _events: axio::IoEvents) {
        panic!("TracepointPerfEvent::register() should not be called");
    }
}

impl PerfEventOps for TracepointPerfEvent {
    fn set_bpf_prog(&mut self, bpf_prog: Arc<dyn FileLike>) -> AxResult<()> {
        static CALLBACK_ID: AtomicUsize = AtomicUsize::new(0);

        let vm = super::bpf::create_basic_ebpf_vm(bpf_prog.clone())?;
        let callback = Box::new(TracePointPerfCallBack::new(bpf_prog, vm));

        let id = CALLBACK_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        self.tp.register_event_callback(id, callback);

        axlog::warn!(
            "Registered BPF program for tracepoint: {}:{} with ID: {}",
            self.tp.system(),
            self.tp.name(),
            id
        );
        // Store the ID in the ebpf_list for later cleanup
        self.ebpf_list.lock().push(id);
        Ok(())
    }

    fn enable(&mut self) -> AxResult<()> {
        axlog::warn!(
            "Enabling tracepoint event: {}:{}",
            self.tp.system(),
            self.tp.name()
        );
        self.tp.enable_event();
        Ok(())
    }

    fn disable(&mut self) -> AxResult<()> {
        self.tp.disable_event();
        Ok(())
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn core::any::Any {
        self
    }
}

impl Drop for TracepointPerfEvent {
    fn drop(&mut self) {
        // Unregister all callbacks associated with this tracepoint event
        let mut ebpf_list = self.ebpf_list.lock();
        for id in ebpf_list.iter() {
            self.tp.unregister_event_callback(*id);
        }
        ebpf_list.clear();
    }
}

/// Creates a new `TracepointPerfEvent` for the given tracepoint ID.
pub fn perf_event_open_tracepoint(args: PerfProbeArgs) -> AxResult<TracepointPerfEvent> {
    let tp_id = match args.config {
        PerfProbeConfig::Raw(tp_id) => tp_id as u32,
        _ => {
            panic!("Invalid PerfProbeConfig for TracepointPerfEvent");
        }
    };
    let tp_manager = crate::tracepoint::tracepoint_manager();
    let tp_map = tp_manager.tracepoint_map();
    let tp = tp_map.get(&tp_id).ok_or(AxError::NotFound)?;
    Ok(TracepointPerfEvent::new(args, tp))
}
