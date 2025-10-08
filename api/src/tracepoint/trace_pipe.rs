use alloc::{format, string::String};
use core::{future::poll_fn, task::Poll};

use axfs_ng_vfs::{VfsError, VfsResult};
use axtask::{
    current,
    future::{block_on, interruptible},
};
use starry_core::task::AsThread;
use tracepoint::TracePipeOps;

use crate::{tracepoint::TRACE_RAW_PIPE, vfs::debug::DebugFsFileOps};

/// File representing the trace pipe.
pub struct TracePipeFile;

impl TracePipeFile {
    fn readable(&self) -> bool {
        let trace_raw_pipe = TRACE_RAW_PIPE.lock();
        !trace_raw_pipe.is_empty()
    }
}

impl DebugFsFileOps for TracePipeFile {
    fn read(&self, buf: &mut [u8], _offset: u64) -> VfsResult<usize> {
        let curr = current();
        let proc_data = &curr.as_thread().proc_data;

        let read_len = loop {
            let mut trace_raw_pipe = TRACE_RAW_PIPE.lock();
            let read_len = super::common_trace_pipe_read(&mut *trace_raw_pipe, buf);
            if read_len != 0 {
                break read_len;
            }
            // Release the lock before waiting
            drop(trace_raw_pipe);
            // wait for new data
            let _result = block_on(interruptible(poll_fn(|cx| {
                if self.readable() {
                    Poll::Ready(true)
                } else {
                    proc_data.child_exit_event.register(cx.waker());
                    Poll::Pending
                }
            })))?;
        };
        Ok(read_len)
    }

    fn write(&self, _buf: &[u8], _offset: u64) -> VfsResult<usize> {
        Err(VfsError::PermissionDenied)
    }
}

/// File representing the "max_record" attribute of the trace command line
/// cache.
pub struct TraceCmdLineSizeFile;

impl DebugFsFileOps for TraceCmdLineSizeFile {
    fn read(&self, buf: &mut [u8], offset: u64) -> VfsResult<usize> {
        let max_record = super::TRACE_CMDLINE_CACHE.lock().max_record();
        let str = format!("{max_record}\n");
        let str_bytes = str.as_bytes();
        let offset = offset as usize;
        if offset >= str_bytes.len() {
            return Ok(0); // Offset is beyond the length of the string
        }
        let len = buf.len().min(str_bytes.len() - offset);
        buf[..len].copy_from_slice(&str_bytes[offset..offset + len]);
        Ok(len)
    }

    fn write(&self, buf: &[u8], _offset: u64) -> VfsResult<usize> {
        let max_record_str = String::from_utf8_lossy(buf);
        let max_record = max_record_str
            .trim()
            .parse()
            .map_err(|_| VfsError::InvalidInput)?;
        super::TRACE_CMDLINE_CACHE.lock().set_max_record(max_record);
        Ok(buf.len())
    }
}
