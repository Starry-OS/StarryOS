use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{any::Any, sync::atomic::AtomicU32};

use axerrno::AxResult;
use axio::Pollable;
use kbpf_basic::perf::{PerfProbeArgs, PerfProbeConfig};
use kprobe::{CallBackFunc, KprobeBuilder, KretprobeBuilder, PtRegs};
use rbpf::EbpfVmRaw;

use crate::{
    file::FileLike,
    kprobe::{
        KernelKprobe, KernelKretprobe, KprobeAuxiliary, register_kprobe, register_kretprobe,
        unregister_kprobe, unregister_kretprobe,
    },
    lock_api::KSpinNoPreempt,
    perf::PerfEventOps,
};

#[derive(Debug)]
pub enum ProbeTy {
    Kprobe(Arc<KernelKprobe>),
    Kretprobe(Arc<KernelKretprobe>),
}

#[derive(Debug)]
pub struct ProbePerfEvent {
    _args: PerfProbeArgs,
    probe: ProbeTy,
    callback_list: Vec<u32>,
}

impl ProbePerfEvent {
    pub fn new(args: PerfProbeArgs, probe: ProbeTy) -> Self {
        ProbePerfEvent {
            _args: args,
            probe,
            callback_list: Vec::new(),
        }
    }
}

impl Drop for ProbePerfEvent {
    fn drop(&mut self) {
        for callback_id in &self.callback_list {
            match self.probe {
                ProbeTy::Kprobe(ref kprobe) => {
                    kprobe.unregister_event_callback(*callback_id);
                }
                ProbeTy::Kretprobe(ref kretprobe) => {
                    kretprobe.unregister_event_callback(*callback_id);
                }
            }
        }
        match self.probe {
            ProbeTy::Kprobe(ref kprobe) => {
                unregister_kprobe(kprobe.clone());
            }
            ProbeTy::Kretprobe(ref kretprobe) => {
                unregister_kretprobe(kretprobe.clone());
            }
        }
    }
}

impl Pollable for ProbePerfEvent {
    fn poll(&self) -> axio::IoEvents {
        axio::IoEvents::empty()
    }

    fn register(&self, _context: &mut core::task::Context<'_>, _events: axio::IoEvents) {
        // do nothing
        todo!()
    }
}

impl PerfEventOps for ProbePerfEvent {
    fn enable(&mut self) -> AxResult<()> {
        axlog::warn!("enabling kprobe/rertprobe");
        match self.probe {
            ProbeTy::Kprobe(ref kprobe) => {
                kprobe.enable();
            }
            ProbeTy::Kretprobe(ref kretprobe) => {
                kretprobe.kprobe().enable();
            }
        }
        Ok(())
    }

    fn disable(&mut self) -> AxResult<()> {
        match self.probe {
            ProbeTy::Kprobe(ref kprobe) => {
                kprobe.disable();
            }
            ProbeTy::Kretprobe(ref kretprobe) => {
                kretprobe.kprobe().disable();
            }
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn set_bpf_prog(&mut self, bpf_prog: Arc<dyn FileLike>) -> AxResult<()> {
        let vm = super::bpf::create_basic_ebpf_vm(bpf_prog.clone())?;

        static CALLBACK_ID: AtomicU32 = AtomicU32::new(0);

        let id = CALLBACK_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        // create a callback to execute the ebpf prog
        let callback = Box::new(KprobePerfCallBack::new(bpf_prog, vm));
        // update callback for kprobe
        match self.probe {
            ProbeTy::Kprobe(ref kprobe) => {
                kprobe.register_event_callback(id, callback);
            }
            ProbeTy::Kretprobe(ref kretprobe) => {
                kretprobe.register_event_callback(id, callback);
            }
        }
        self.callback_list.push(id);
        Ok(())
    }
}

pub struct KprobePerfCallBack {
    _bpf_prog_file: Arc<dyn FileLike>,
    vm: EbpfVmRaw<'static>,
}

unsafe impl Send for KprobePerfCallBack {}
unsafe impl Sync for KprobePerfCallBack {}

impl KprobePerfCallBack {
    fn new(bpf_prog_file: Arc<dyn FileLike>, vm: EbpfVmRaw<'static>) -> Self {
        Self {
            _bpf_prog_file: bpf_prog_file,
            vm,
        }
    }
}

impl CallBackFunc for KprobePerfCallBack {
    fn call(&self, pt_regs: &mut PtRegs) {
        axlog::trace!("PtRegs in kprobe callback: {:#x?}", pt_regs);
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

fn perf_probe_arg_to_kretprobe_builder(
    args: &PerfProbeArgs,
) -> KretprobeBuilder<KSpinNoPreempt<()>> {
    let symbol = &args.name;
    let addr = crate::vfs::KALLSYMS
        .get()
        .and_then(|ksym| ksym.lookup_name(symbol))
        .unwrap() as usize;
    axlog::warn!("perf_probe: symbol: {}, addr: {:#x}", symbol, addr);
    let builder = KretprobeBuilder::<KSpinNoPreempt<()>>::new(Some(symbol.clone()), addr, 10);
    builder
}

pub fn perf_event_open_kprobe(args: PerfProbeArgs) -> ProbePerfEvent {
    let symbol = &args.name;
    axlog::warn!("create kprobe for symbol: {symbol}");

    let probe = match args.config {
        PerfProbeConfig::Raw(val) => {
            if val == 0 {
                // kprobe
                let builder = perf_probe_arg_to_kprobe_builder(&args);
                let kprobe = register_kprobe(builder);
                ProbeTy::Kprobe(kprobe)
            } else if val == 1 {
                // kretprobe
                let builder = perf_probe_arg_to_kretprobe_builder(&args);
                let kretprobe = register_kretprobe(builder);
                ProbeTy::Kretprobe(kretprobe)
            } else {
                panic!("unsupported config for kprobe");
            }
        }
        _ => {
            panic!("unsupported config for kprobe");
        }
    };

    axlog::warn!("create kprobe ok");
    ProbePerfEvent::new(args, probe)
}
