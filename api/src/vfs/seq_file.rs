use alloc::{string::String, sync::Arc};
use core::{any::Any, cmp::min, fmt, task::Context, time::Duration};

use axfs_ng_vfs::{
    DeviceId, FileNodeOps, FilesystemOps, Metadata, MetadataUpdate, NodeFlags, NodeOps,
    NodePermission, NodeType, VfsError, VfsResult,
};
use axpoll::{IoEvents, Pollable};
use axsync::Mutex;

/// Internal buffer size for SeqFile (4KB page).
const SEQ_BUF_SIZE: usize = 0x1000;

/// Report a large virtual file size (e.g., 1MB) to the VFS.
const VIRTUAL_FILE_SIZE: u64 = 1024 * 1024;

/// The interface that specific features (e.g., /proc/maps) must implement.
///
/// This trait separates the "Business Logic" (traversal) from the "IO Logic" (buffering).
pub trait SeqIterator: Send + 'static {
    /// The type of the object being iterated
    type Item;

    /// Initialize the iterator and return the first item.
    fn start(&mut self) -> Option<Self::Item>;

    /// Move to the next item.
    fn next(&mut self) -> Option<Self::Item>;

    /// Format the current item into the buffer.
    fn show(&self, item: &Self::Item, buf: &mut String) -> fmt::Result;
}

/// A generic adapter that manages the state of sequential file reading.
/// It handles buffering and offset tracking internally.
pub struct SeqFile<I: SeqIterator> {
    iter: I,
    buf: String,
    buf_read_pos: usize,
    last_file_offset: u64,
    is_eof: bool,
}

impl<I: SeqIterator> SeqFile<I> {
    pub fn new(iter: I) -> Self {
        Self {
            iter,
            buf: String::with_capacity(SEQ_BUF_SIZE),
            buf_read_pos: 0,
            last_file_offset: 0,
            is_eof: false,
        }
    }

    pub fn read(&mut self, output: &mut [u8], offset: u64) -> VfsResult<usize> {
        output.fill(0);
        // Consistency Check & Reset Logic
        if offset == 0 {
            self.reset();
        } else if offset != self.last_file_offset {
            // Random seek or concurrent read race detected.
            // If backward seek, reset. Forward seeking into holes is not supported.
            if offset < self.last_file_offset {
                self.reset();
                if offset > 0 {
                    // Linear scan to catch up is not implemented for V1.
                    return Err(VfsError::InvalidInput);
                }
            } else {
                return Err(VfsError::InvalidInput);
            }
        }

        let mut total_written = 0;
        let mut output_cursor = 0;
        let output_len = output.len();

        //  Main Filling Loop
        while output_cursor < output_len {
            // Flush internal buffer if it has data
            let available = self.buf.len() - self.buf_read_pos;
            if available > 0 {
                let to_copy = min(available, output_len - output_cursor);
                output[output_cursor..output_cursor + to_copy].copy_from_slice(
                    &self.buf.as_bytes()[self.buf_read_pos..self.buf_read_pos + to_copy],
                );

                output_cursor += to_copy;
                self.buf_read_pos += to_copy;
                self.last_file_offset += to_copy as u64;
                total_written += to_copy;
                continue;
            }

            // Buffer empty, fetch next item
            if self.is_eof {
                break;
            }

            self.buf.clear();
            self.buf_read_pos = 0;

            // Heuristic: if at offset 0 and haven't started, call start(), else next()
            let next_item = if self.last_file_offset == 0 && !self.has_started() {
                self.iter.start()
            } else {
                self.iter.next()
            };

            match next_item {
                Some(item) => {
                    // Format into internal buffer
                    self.iter
                        .show(&item, &mut self.buf)
                        .map_err(|_| VfsError::Io)?;
                    // Handle case where show() produces empty string (rare but possible)
                    if self.buf.is_empty() {
                        continue;
                    }
                }
                None => {
                    self.is_eof = true;
                    break;
                }
            }
        }

        Ok(total_written)
    }

    fn reset(&mut self) {
        self.buf.clear();
        self.buf_read_pos = 0;
        self.last_file_offset = 0;
        self.is_eof = false;
    }

    fn has_started(&self) -> bool {
        self.is_eof || self.last_file_offset > 0 || !self.buf.is_empty()
    }
}

/// A VFS Node wrapper for SeqFile.
/// This structure implements NodeOps, Pollable, and FileNodeOps so it can be mounted in VFS.
pub struct SeqFileNode<I: SeqIterator> {
    inner: Mutex<SeqFile<I>>,
    fs: Arc<dyn FilesystemOps>,
    inode: u64,
}

impl<I: SeqIterator> SeqFileNode<I> {
    /// Create a new VFS node for the SeqFile.
    /// `fs`: The parent filesystem (e.g., procfs).
    /// `inode`: The inode number assigned to this file.
    pub fn new(iter: I, fs: Arc<dyn FilesystemOps>, inode: u64) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(SeqFile::new(iter)),
            fs,
            inode,
        })
    }
}

// === VFS Traits Implementation ===

impl<I: SeqIterator> NodeOps for SeqFileNode<I> {
    fn inode(&self) -> u64 {
        self.inode
    }

    fn metadata(&self) -> VfsResult<Metadata> {
        Ok(Metadata {
            device: 0,
            inode: self.inode,
            nlink: 1,
            mode: NodePermission::from_bits_truncate(0o444), // Read-only
            node_type: NodeType::RegularFile,
            uid: 0,
            gid: 0,
            size: VIRTUAL_FILE_SIZE, /* Hack: Set a fake non-zero size to ensure tools like 'cat' read the file. */
            block_size: 0,
            blocks: 0,
            rdev: DeviceId::default(),
            atime: Duration::default(),
            mtime: Duration::default(),
            ctime: Duration::default(),
        })
    }

    fn update_metadata(&self, _update: MetadataUpdate) -> VfsResult<()> {
        Ok(())
    }

    fn filesystem(&self) -> &dyn FilesystemOps {
        self.fs.as_ref()
    }

    fn sync(&self, _data_only: bool) -> VfsResult<()> {
        Ok(())
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }

    fn flags(&self) -> NodeFlags {
        NodeFlags::empty()
    }
}

impl<I: SeqIterator> Pollable for SeqFileNode<I> {
    fn poll(&self) -> IoEvents {
        // Always readable, never writable (logic-wise)
        IoEvents::IN | IoEvents::OUT
    }

    fn register(&self, _context: &mut Context<'_>, _events: IoEvents) {}
}

impl<I: SeqIterator> FileNodeOps for SeqFileNode<I> {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> VfsResult<usize> {
        self.inner.lock().read(buf, offset)
    }

    fn write_at(&self, _buf: &[u8], _offset: u64) -> VfsResult<usize> {
        Err(VfsError::PermissionDenied)
    }

    fn append(&self, _buf: &[u8]) -> VfsResult<(usize, u64)> {
        Err(VfsError::PermissionDenied)
    }

    fn set_len(&self, _len: u64) -> VfsResult<()> {
        Err(VfsError::PermissionDenied)
    }

    fn set_symlink(&self, _target: &str) -> VfsResult<()> {
        Err(VfsError::PermissionDenied)
    }
}
