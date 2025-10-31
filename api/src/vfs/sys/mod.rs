use alloc::format;

use axconfig::plat::CPU_NUM;
use axerrno::LinuxResult;
use axfs_ng::{FsContext, OpenOptions};
use axfs_ng_vfs::path::{Path, PathBuf};

use crate::vfs::DIR_PERMISSION;

fn create_dir(fs: &FsContext, path_str: &str) -> LinuxResult<PathBuf> {
    let mut path = PathBuf::new();
    for comp in Path::new(path_str).components() {
        path.push(comp.as_str());
        if fs.resolve(&path).is_err() {
            fs.create_dir(&path, DIR_PERMISSION)?;
        }
    }
    Ok(path)
}

pub fn init_sysfs(fs: &FsContext) -> LinuxResult<()> {
    // /sys/devices/system/cpu/
    // /sys/bus/event_source/devices/kprbe
    let cpu_path = create_dir(fs, "/sys/devices/system/cpu")?;
    let kprobe_path = create_dir(fs, "/sys/bus/event_source/devices/kprobe")?;
    let kprobe_fmt = create_dir(fs, "/sys/bus/event_source/devices/kprobe/format")?;
    let online_cpu = format!("0-{}\n", CPU_NUM - 1);
    let online = OpenOptions::new()
        .create(true)
        .write(true)
        .open(fs, cpu_path.join("online"))?
        .into_file()?;
    online.write_at(&mut online_cpu.as_bytes(), 0)?;

    let possible = OpenOptions::new()
        .create(true)
        .write(true)
        .open(fs, cpu_path.join("possible"))?
        .into_file()?;
    possible.write_at(&mut online_cpu.as_bytes(), 0)?;

    let kprobe_type = OpenOptions::new()
        .create(true)
        .write(true)
        .open(fs, kprobe_path.join("type"))?
        .into_file()?;
    kprobe_type.write_at(&mut b"6\n".as_ref(), 0)?;

    let retprobe = OpenOptions::new()
        .create(true)
        .write(true)
        .open(fs, kprobe_fmt.join("retprobe"))?
        .into_file()?;
    retprobe.write_at(&mut b"config:0\n".as_ref(), 0)?;
    Ok(())
}
