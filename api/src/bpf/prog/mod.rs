use alloc::sync::Arc;
use core::{fmt::Debug, panic};

use axerrno::AxResult;
use axio::Pollable;
use kbpf_basic::{
    EBPFPreProcessor,
    linux_bpf::bpf_attr,
    prog::{BpfProgMeta, BpfProgVerifierInfo},
};

use crate::{
    bpf::{
        map::BpfMap,
        tansform::{EbpfKernelAuxiliary, bpferror_to_axerr},
    },
    file::{FileLike, add_file_like},
};
pub struct BpfProg {
    meta: BpfProgMeta,
    preprocessor: EBPFPreProcessor,
}

impl Debug for BpfProg {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BpfProg").field("meta", &self.meta).finish()
    }
}

impl BpfProg {
    pub fn new(meta: BpfProgMeta, preprocessor: EBPFPreProcessor) -> Self {
        Self { meta, preprocessor }
    }

    pub fn insns(&self) -> &[u8] {
        &self.preprocessor.get_new_insn()
    }
}

impl Drop for BpfProg {
    fn drop(&mut self) {
        unsafe {
            for ptr in self.preprocessor.get_raw_file_ptr() {
                let file = Arc::from_raw(*ptr as *const u8 as *const BpfMap);
                drop(file)
            }
        }
    }
}

impl Pollable for BpfProg {
    fn poll(&self) -> axio::IoEvents {
        panic!("BpfProg::poll() should not be called");
    }

    fn register(&self, _context: &mut core::task::Context<'_>, _events: axio::IoEvents) {
        panic!("BpfProg::register() should not be called");
    }
}

impl FileLike for BpfProg {
    fn read(&self, _dst: &mut crate::file::SealedBufMut) -> axio::Result<usize> {
        panic!("BpfProg::read() should not be called");
    }

    fn write(&self, _src: &mut crate::file::SealedBuf) -> axio::Result<usize> {
        panic!("BpfProg::write() should not be called");
    }

    fn stat(&self) -> axio::Result<crate::file::Kstat> {
        Ok(crate::file::Kstat::default())
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn core::any::Any + Send + Sync> {
        self
    }

    fn path(&self) -> alloc::borrow::Cow<str> {
        "anon_inode:[bpf_prog]".into()
    }
}

/// Load a BPF program into the kernel.
///
/// See https://ebpf-docs.dylanreimerink.nl/linux/syscall/BPF_PROG_LOAD/
pub fn bpf_prog_load(attr: &bpf_attr) -> AxResult<isize> {
    let mut args =
        BpfProgMeta::try_from_bpf_attr::<EbpfKernelAuxiliary>(attr).map_err(bpferror_to_axerr)?;
    axlog::warn!("bpf_prog_load: {:#?}", args);
    let _log_info = BpfProgVerifierInfo::from(attr);
    let prog_insn = args.take_insns().unwrap();
    let preprocessor =
        EBPFPreProcessor::preprocess::<EbpfKernelAuxiliary>(prog_insn).expect("preprocess failed");
    let prog = Arc::new(BpfProg::new(args, preprocessor));

    let fd = add_file_like(prog, false)?;

    axlog::warn!("bpf_prog_load: fd: {}", fd);
    Ok(fd as _)
}
