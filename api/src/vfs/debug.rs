use alloc::sync::Arc;
use core::{any::Any, task::Context};

use axfs_ng_vfs::{
    FileNodeOps, Filesystem, FilesystemOps, Metadata, MetadataUpdate, NodeFlags, NodeOps,
    NodePermission, NodeType, VfsError, VfsResult,
};
use axio::{IoEvents, Pollable};
use inherit_methods_macro::inherit_methods;
use starry_core::vfs::{DirMaker, DirMapping, SimpleDir, SimpleFs, SimpleFsNode};

/// Operations for a debugfs file.
pub trait DebugFsFileOps: Send + Sync + 'static {
    /// Reads the entire content of the file.
    fn read(&self, buf: &mut [u8], offset: u64) -> VfsResult<usize>;
    /// Writes data to the file, replacing its entire content.
    fn write(&self, buf: &[u8], offset: u64) -> VfsResult<usize>;
}

pub struct DebugFsFile {
    node: SimpleFsNode,
    ops: Arc<dyn DebugFsFileOps + Send + Sync>,
}

impl DebugFsFile {
    /// Creates a simple file from given file operations.
    fn new(fs: Arc<SimpleFs>, ty: NodeType, ops: impl DebugFsFileOps) -> Arc<Self> {
        let node = SimpleFsNode::new(fs, ty, NodePermission::default());
        Arc::new(Self {
            node,
            ops: Arc::new(ops),
        })
    }

    /// Creates a simple file from given file operations.
    pub fn new_regular(fs: Arc<SimpleFs>, ops: impl DebugFsFileOps) -> Arc<Self> {
        Self::new(fs, NodeType::RegularFile, ops)
    }
}

#[inherit_methods(from = "self.node")]
impl NodeOps for DebugFsFile {
    fn inode(&self) -> u64;

    fn metadata(&self) -> VfsResult<Metadata>;

    fn update_metadata(&self, update: MetadataUpdate) -> VfsResult<()>;

    fn filesystem(&self) -> &dyn FilesystemOps;

    fn sync(&self, data_only: bool) -> VfsResult<()>;

    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }

    fn len(&self) -> VfsResult<u64>;

    fn flags(&self) -> NodeFlags {
        NodeFlags::NON_CACHEABLE
    }
}

impl FileNodeOps for DebugFsFile {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> VfsResult<usize> {
        self.ops.read(buf, offset)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> VfsResult<usize> {
        self.ops.write(buf, offset)
    }

    fn append(&self, _buf: &[u8]) -> VfsResult<(usize, u64)> {
        Err(VfsError::OperationNotSupported)
    }

    fn set_len(&self, _len: u64) -> VfsResult<()> {
        Ok(())
    }

    fn set_symlink(&self, _target: &str) -> VfsResult<()> {
        Err(VfsError::OperationNotSupported)
    }

    fn ioctl(&self, _cmd: u32, _arg: usize) -> VfsResult<usize> {
        Err(VfsError::BadIoctl)
    }
}

impl Pollable for DebugFsFile {
    fn poll(&self) -> IoEvents {
        IoEvents::IN | IoEvents::OUT
    }

    fn register(&self, _context: &mut Context<'_>, _events: IoEvents) {}
}

/// Create a new debugfs filesystem.
pub fn new_debugfs() -> Filesystem {
    // TODO: update fs_type
    SimpleFs::new_with("debug".into(), 0xffff, debugfs_builder)
}

fn debugfs_builder(fs: Arc<SimpleFs>) -> DirMaker {
    let mut root = DirMapping::new();
    root.add("tracing", tracing_dir(fs.clone()));
    SimpleDir::new_maker(fs, Arc::new(root))
}

fn tracing_dir(fs: Arc<SimpleFs>) -> DirMaker {
    let mut tracing_root = DirMapping::new();
    tracing_root.set_cacheable(false);
    // See crate::tracepoint::TRACE_CMDLINE_CACHE
    tracing_root.add(
        "saved_cmdlines_size",
        DebugFsFile::new_regular(fs.clone(), crate::tracepoint::TraceCmdLineSizeFile),
    );
    tracing_root.add_dynamic("saved_cmdlines", {
        let fs = fs.clone();
        move || {
            let f = crate::tracepoint::dynamic_create_cmdline();
            DebugFsFile::new_regular(fs.clone(), f).into()
        }
    });
    tracing_root.add(
        "trace_pipe",
        DebugFsFile::new_regular(fs.clone(), crate::tracepoint::TracePipeFile),
    );
    tracing_root.add_dynamic("trace", {
        let fs = fs.clone();
        move || {
            let f = crate::tracepoint::dynamic_create_trace();
            DebugFsFile::new_regular(fs.clone(), f).into()
        }
    });
    tracing_root.add("events", crate::tracepoint::init_events(fs.clone()));
    SimpleDir::new_maker(fs, Arc::new(tracing_root))
}
