//! Basic virtual filesystem support

pub mod debug;
pub mod dev;
mod device;
mod dir;
mod file;
mod fs;
mod proc;
pub mod sys;
mod tmp;

use alloc::sync::Arc;

use axerrno::LinuxResult;
use axfs::{FS_CONTEXT, FsContext};
use axfs_ng_vfs::{
    DirNodeOps, FileNodeOps, Filesystem, NodePermission, WeakDirEntry,
    path::{Path, PathBuf},
};
use ksym::KallsymsMapped;
pub use tmp::MemoryFs;

pub use self::{device::*, dir::*, file::*, fs::*, proc::KALLSYMS};

/// A callback that builds a `Arc<dyn DirNodeOps>` for a given
/// `WeakDirEntry`.
pub type DirMaker = Arc<dyn Fn(WeakDirEntry) -> Arc<dyn DirNodeOps> + Send + Sync>;

/// An enum containing either a directory ([`DirMaker`]) or a file (`Arc<dyn
/// FileNodeOps>`).
#[derive(Clone)]
pub enum NodeOpsMux {
    /// A directory node.
    Dir(DirMaker),
    /// A file node.
    File(Arc<dyn FileNodeOps>),
}

impl From<DirMaker> for NodeOpsMux {
    fn from(maker: DirMaker) -> Self {
        Self::Dir(maker)
    }
}

impl<T: FileNodeOps> From<Arc<T>> for NodeOpsMux {
    fn from(ops: Arc<T>) -> Self {
        Self::File(ops)
    }
}

#[derive(Clone)]
enum NodeOpsMuxTy {
    Static(NodeOpsMux),
    Dynamic(Arc<dyn Fn() -> NodeOpsMux + Send + Sync>),
}

const DIR_PERMISSION: NodePermission = NodePermission::from_bits_truncate(0o755);

fn mount_at(fs: &FsContext, path: &str, mount_fs: Filesystem) -> LinuxResult<()> {
    if fs.resolve(path).is_err() {
        fs.create_dir(path, DIR_PERMISSION)?;
    }
    fs.resolve(path)?.mount(&mount_fs)?;
    info!("Mounted {} at {}", mount_fs.name(), path);
    Ok(())
}

fn read_kallsyms() -> LinuxResult<KallsymsMapped<'static>> {
    let kallsyms_start = __kallsyms_start as *const () as usize;
    let kallsyms_end = __kallsyms_end as *const () as usize;
    let kallsyms_size = kallsyms_end - kallsyms_start;
    let kallsyms =
        unsafe { core::slice::from_raw_parts(__kallsyms_start as *const u8, kallsyms_size) }
            .to_vec();

    // SAFETY: We assume that the kallsyms section is valid and won't cause undefined behavior when accessed.
    axalloc::global_add_memory(kallsyms_start, kallsyms_size).unwrap();

    axlog::info!("Read kallsyms, size: {}KB", kallsyms.len() / 1024);
    let kallsyms = kallsyms.leak();
    let ksym = ksym::KallsymsMapped::from_blob(
        kallsyms,
        _stext as *const () as u64,
        _etext as *const () as u64,
    )
    .expect("Failed to create KallsymsMapped");
    axlog::info!(
        "find addr of _stext: {:#x}",
        ksym.lookup_name("_start").unwrap_or(0)
    );
    Ok(ksym)
}

unsafe extern "C" {
    fn _stext();
    fn _etext();
    fn __kallsyms_start();
    fn __kallsyms_end();
}

/// Mount all filesystems
pub fn mount_all() -> LinuxResult<()> {
    info!("Initialize pseudofs...");
    let ksym = read_kallsyms()?;
    let fs = FS_CONTEXT.lock();
    mount_at(&fs, "/dev", dev::new_devfs())?;
    mount_at(&fs, "/dev/shm", tmp::MemoryFs::new())?;
    mount_at(&fs, "/tmp", tmp::MemoryFs::new())?;
    mount_at(&fs, "/proc", proc::new_procfs(ksym))?;

    mount_at(&fs, "/sys", tmp::MemoryFs::new())?;
    let mut path = PathBuf::new();
    for comp in Path::new("/sys/class/graphics/fb0/device").components() {
        path.push(comp.as_str());
        if fs.resolve(&path).is_err() {
            fs.create_dir(&path, DIR_PERMISSION)?;
        }
    }
    path.push("subsystem");
    fs.symlink("whatever", &path)?;

    sys::init_sysfs(&fs)?;

    // for debugfs
    let mut path = PathBuf::new();
    for comp in Path::new("/sys/kernel/debug").components() {
        path.push(comp.as_str());
        if fs.resolve(&path).is_err() {
            fs.create_dir(&path, DIR_PERMISSION)?;
        }
    }

    mount_at(&fs, "/sys/kernel/debug", debug::new_debugfs())?;

    drop(fs);

    #[cfg(feature = "dev-log")]
    dev::bind_dev_log().expect("Failed to bind /dev/log");

    Ok(())
}
