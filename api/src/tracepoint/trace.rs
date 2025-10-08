use alloc::{format, string::String};

use axfs_ng_vfs::VfsResult;
use kspin::SpinRaw;
use tracepoint::{TraceCmdLineCacheSnapshot, TracePipeSnapshot};

use crate::vfs::debug::DebugFsFileOps;
pub struct TraceFile(SpinRaw<TracePipeSnapshot>);

impl DebugFsFileOps for TraceFile {
    fn read(&self, buf: &mut [u8], offset: u64) -> VfsResult<usize> {
        let offset = offset as usize;
        let default_fmt_str = self.0.lock().default_fmt_str();
        if offset >= default_fmt_str.len() {
            let mut snapshot = self.0.lock();
            Ok(super::common_trace_pipe_read(&mut *snapshot, buf))
        } else {
            let len = buf.len().min(default_fmt_str.len() - offset);
            buf[..len].copy_from_slice(&default_fmt_str.as_bytes()[offset..offset + len]);
            Ok(len)
        }
    }

    fn write(&self, buf: &[u8], _offset: u64) -> VfsResult<usize> {
        if buf.len() == 1 {
            let mut trace_raw_pipe = super::TRACE_RAW_PIPE.lock();
            trace_raw_pipe.clear();
        }
        Ok(buf.len())
    }
}

pub fn dynamic_create_trace() -> TraceFile {
    let snapshot = super::TRACE_RAW_PIPE.lock().snapshot();
    TraceFile(SpinRaw::new(snapshot))
}

pub struct TraceCmdLineFile(SpinRaw<TraceCmdLineCacheSnapshot>);

impl DebugFsFileOps for TraceCmdLineFile {
    fn read(&self, buf: &mut [u8], _offset: u64) -> VfsResult<usize> {
        let mut copy_len = 0;
        let mut peek_flag = false;
        let mut snapshot = self.0.lock();
        loop {
            if let Some((pid, cmdline)) = snapshot.peek() {
                let record_str = format!("{} {}\n", pid, String::from_utf8_lossy(cmdline));
                if copy_len + record_str.len() > buf.len() {
                    break;
                }
                let len = record_str.len();
                buf[copy_len..copy_len + len].copy_from_slice(record_str.as_bytes());
                copy_len += len;
                peek_flag = true;
            }
            if peek_flag {
                snapshot.pop(); // Remove the record after reading
                peek_flag = false;
            } else {
                break; // No more records to read
            }
        }
        Ok(copy_len)
    }

    fn write(&self, _buf: &[u8], _offset: u64) -> VfsResult<usize> {
        Err(axfs_ng_vfs::VfsError::PermissionDenied)
    }
}

pub fn dynamic_create_cmdline() -> TraceCmdLineFile {
    let snapshot = super::TRACE_CMDLINE_CACHE.lock().snapshot();
    TraceCmdLineFile(SpinRaw::new(snapshot))
}


