use alloc::sync::Arc;
use core::{fmt::Debug, panic};

use axpoll::Pollable;
use kbpf_basic::{EBPFPreProcessor, prog::BpfProgMeta};

use crate::{bpf::map::BpfMap, file::FileLike};
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
    fn poll(&self) -> axpoll::IoEvents {
        panic!("BpfProg::poll() should not be called");
    }

    fn register(&self, _context: &mut core::task::Context<'_>, _events: axpoll::IoEvents) {
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
