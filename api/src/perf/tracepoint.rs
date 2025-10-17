use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::sync::atomic::AtomicUsize;

use axerrno::{AxError, AxResult};
use axio::Pollable;
use kbpf_basic::perf::{PerfProbeArgs, PerfProbeConfig};
use kspin::SpinNoPreempt;
use rbpf::EbpfVmRaw;
use tracepoint::{TracePoint, TracePointCallBackFunc};

use crate::{
    bpf::{BPF_HELPER_FUN_SET, prog::BpfProg},
    file::FileLike,
    lock_api::KSpinNoPreempt,
    perf::PerfEventOps,
    tracepoint::KernelTraceAux,
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
    _bpf_prog_file: Arc<BpfProg>,
    vm: EbpfVmRaw<'static>,
}

impl TracePointPerfCallBack {
    pub fn new(bpf_prog_file: Arc<BpfProg>, vm: EbpfVmRaw<'static>) -> Self {
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

        let bpf_prog = bpf_prog.into_any().downcast::<BpfProg>().unwrap();
        let prog_slice = bpf_prog.insns();

        let prog_slice =
            unsafe { core::slice::from_raw_parts(prog_slice.as_ptr(), prog_slice.len()) };
        let mut vm = EbpfVmRaw::new(Some(prog_slice)).map_err(|e| {
            axlog::error!("create ebpf vm failed: {:?}", e);
            AxError::InvalidInput
        })?;
        for (key, value) in BPF_HELPER_FUN_SET.iter() {
            vm.register_helper(*key, *value).unwrap();
        }

        // create a callback to execute the ebpf prog
        vm.register_allowed_memory(0..u64::MAX);
        let callback = Box::new(TracePointPerfCallBack::new(bpf_prog, vm));

        let id = CALLBACK_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        self.tp.register_raw_callback(id, callback);

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
        self.tp.enable();
        Ok(())
    }

    fn disable(&mut self) -> AxResult<()> {
        self.tp.disable();
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
            self.tp.unregister_raw_callback(*id);
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
