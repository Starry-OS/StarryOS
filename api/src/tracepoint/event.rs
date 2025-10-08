use axfs_ng_vfs::{VfsError, VfsResult};
use ringbuf::Arc;
use tracepoint::EventInfo;

use crate::{lock_api::KSpinNoPreempt, tracepoint::KernelTraceAux, vfs::debug::DebugFsFileOps};

/// File representing the "enable" attribute of a tracepoint event.
pub struct EventEnableFile(Arc<EventInfo<KSpinNoPreempt<()>, KernelTraceAux>>);

impl EventEnableFile {
    /// Create a new `EventEnableFile` instance.
    pub fn new(tracepoint_info: Arc<EventInfo<KSpinNoPreempt<()>, KernelTraceAux>>) -> Self {
        EventEnableFile(tracepoint_info)
    }
}

impl DebugFsFileOps for EventEnableFile {
    fn read(&self, buf: &mut [u8], offset: u64) -> VfsResult<usize> {
        let tracepoint_info = &self.0;
        let enable_value = tracepoint_info.enable_file().read();
        let offset = offset as usize;
        if offset >= enable_value.len() {
            return Ok(0); // Offset is beyond the length of the string
        }
        let len = buf.len().min(enable_value.len() - offset);
        buf[..len].copy_from_slice(&enable_value.as_bytes()[offset..offset + len]);
        Ok(len)
    }

    fn write(&self, buf: &[u8], _offset: u64) -> VfsResult<usize> {
        let tracepoint_info = &self.0;
        if buf.is_empty() {
            return Err(VfsError::InvalidInput);
        }
        tracepoint_info.enable_file().write(buf[0] as _);
        Ok(buf.len())
    }
}
