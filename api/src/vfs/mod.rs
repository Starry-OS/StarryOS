//! Virtual filesystems

pub mod debug;
pub mod dev;
mod proc;
mod tmp;

use alloc::string::String;

use axerrno::LinuxResult;
use axfs_ng::{FS_CONTEXT, FsContext, OpenOptions};
use axfs_ng_vfs::{
    Filesystem, NodePermission,
    path::{Path, PathBuf},
};
pub use starry_core::vfs::{Device, DeviceOps, DirMapping, SimpleFs};
pub use tmp::MemoryFs;

const DIR_PERMISSION: NodePermission = NodePermission::from_bits_truncate(0o755);

fn mount_at(fs: &FsContext, path: &str, mount_fs: Filesystem) -> LinuxResult<()> {
    if fs.resolve(path).is_err() {
        fs.create_dir(path, DIR_PERMISSION)?;
    }
    fs.resolve(path)?.mount(&mount_fs)?;
    info!("Mounted {} at {}", mount_fs.name(), path);
    Ok(())
}

fn read_kallsyms() -> LinuxResult<String> {
    let file = OpenOptions::new()
        .read(true)
        .open(&FS_CONTEXT.lock(), "/root/kallsyms")?
        .into_file()?;

    let mut kallsyms = String::new();
    let mut buf = [0; 4096];
    let mut offset = 0;
    loop {
        let n = file.read_at(&mut buf.as_mut_slice(), offset)?;
        if n == 0 {
            break;
        }
        kallsyms.push_str(core::str::from_utf8(&buf[..n]).unwrap());
        offset += n as u64;
    }
    Ok(kallsyms)
}

/// Mount all filesystems
pub fn mount_all() -> LinuxResult<()> {
    let kallsyms = read_kallsyms()?;
    ksym::init_kernel_symbols(&kallsyms);
    ax_println!(
        "find addr of _stext: {:#x}",
        ksym::addr_from_symbol("_start").unwrap_or(0)
    );
    let fs = FS_CONTEXT.lock();
    mount_at(&fs, "/dev", dev::new_devfs())?;
    mount_at(&fs, "/dev/shm", tmp::MemoryFs::new())?;
    mount_at(&fs, "/tmp", tmp::MemoryFs::new())?;
    mount_at(&fs, "/proc", proc::new_procfs(kallsyms))?;

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
