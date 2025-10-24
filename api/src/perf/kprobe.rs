use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{any::Any, sync::atomic::AtomicU32};

use axerrno::AxResult;
use axio::Pollable;
use kbpf_basic::perf::PerfProbeArgs;
use kprobe::{CallBackFunc, KprobeBuilder, PtRegs};
use rbpf::EbpfVmRaw;

use crate::{
    bpf::{BPF_HELPER_FUN_SET, prog::BpfProg},
    file::FileLike,
    kprobe::{KernelKprobe, KprobeAuxiliary, register_kprobe, unregister_kprobe},
    perf::PerfEventOps,
};

#[derive(Debug)]
pub struct KprobePerfEvent {
    _args: PerfProbeArgs,
    kprobe: Arc<KernelKprobe>,
    callback_list: Vec<u32>,
}

impl KprobePerfEvent {
    pub fn new(args: PerfProbeArgs, kprobe: Arc<KernelKprobe>) -> Self {
        KprobePerfEvent {
            _args: args,
            kprobe,
            callback_list: Vec::new(),
        }
    }
}

impl Drop for KprobePerfEvent {
    fn drop(&mut self) {
        for callback_id in &self.callback_list {
            self.kprobe.unregister_event_callback(*callback_id);
        }
        unregister_kprobe(self.kprobe.clone());
    }
}

impl Pollable for KprobePerfEvent {
    fn poll(&self) -> axio::IoEvents {
        axio::IoEvents::empty()
    }

    fn register(&self, _context: &mut core::task::Context<'_>, _events: axio::IoEvents) {
        // do nothing
        todo!()
    }
}

impl PerfEventOps for KprobePerfEvent {
    fn enable(&mut self) -> AxResult<()> {
        self.kprobe.enable();
        Ok(())
    }

    fn disable(&mut self) -> AxResult<()> {
        self.kprobe.disable();
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn set_bpf_prog(&mut self, bpf_prog: Arc<dyn FileLike>) -> AxResult<()> {
        let bpf_prog = bpf_prog.into_any().downcast::<BpfProg>().unwrap();
        let prog = bpf_prog.insns();
        let prog = unsafe { core::slice::from_raw_parts_mut(prog.as_ptr() as *mut u8, prog.len()) };
        let mut vm = EbpfVmRaw::new(Some(prog)).unwrap();
        for (key, value) in BPF_HELPER_FUN_SET.iter() {
            vm.register_helper(*key, *value).unwrap();
        }
        vm.register_allowed_memory(0..u64::MAX);
        static CALLBACK_ID: AtomicU32 = AtomicU32::new(0);

        let id = CALLBACK_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        // create a callback to execute the ebpf prog
        let callback = Box::new(KprobePerfCallBack::new(bpf_prog, vm));
        // update callback for kprobe
        self.kprobe.register_event_callback(id, callback);
        self.callback_list.push(id);
        Ok(())
    }
}

pub struct KprobePerfCallBack {
    _bpf_prog_file: Arc<BpfProg>,
    vm: EbpfVmRaw<'static>,
}

unsafe impl Send for KprobePerfCallBack {}
unsafe impl Sync for KprobePerfCallBack {}

impl KprobePerfCallBack {
    fn new(bpf_prog_file: Arc<BpfProg>, vm: EbpfVmRaw<'static>) -> Self {
        Self {
            _bpf_prog_file: bpf_prog_file,
            vm,
        }
    }
}

impl CallBackFunc for KprobePerfCallBack {
    fn call(&self, pt_regs: &mut PtRegs) {
        let probe_context = unsafe {
            core::slice::from_raw_parts_mut(pt_regs as *mut PtRegs as *mut u8, size_of::<PtRegs>())
        };
        let res = self.vm.execute_program(probe_context);
        if res.is_err() {
            axlog::error!("kprobe callback error: {:?}", res);
        }
    }
}

fn perf_probe_arg_to_kprobe_builder(args: &PerfProbeArgs) -> KprobeBuilder<KprobeAuxiliary> {
    let symbol = &args.name;
    let addr = crate::vfs::KALLSYMS
        .get()
        .and_then(|ksym| ksym.lookup_name(symbol))
        .unwrap() as usize;
    // let addr = syscall_entry as usize;
    axlog::warn!("perf_probe: symbol: {}, addr: {:#x}", symbol, addr);
    let builder = KprobeBuilder::new(Some(symbol.clone()), addr, 0, false);
    builder
}

pub fn perf_event_open_kprobe(args: PerfProbeArgs) -> KprobePerfEvent {
    let symbol = &args.name;
    axlog::warn!("create kprobe for symbol: {symbol}");
    let builder = perf_probe_arg_to_kprobe_builder(&args);
    let kprobe = register_kprobe(builder);
    axlog::warn!("create kprobe ok");
    KprobePerfEvent::new(args, kprobe)
}
