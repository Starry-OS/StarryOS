use core::ffi::c_char;

use axerrno::{AxError, AxResult};

use crate::{
    file::{InotifyFlags, InotifyInstance, add_file_like, get_file_like},
    mm::vm_load_string,
};
pub fn sys_inotify_init1(flags: i32) -> AxResult<isize> {
    let instance = InotifyInstance::new(flags)?;

    // Use add_file_like to add file descriptor
    let cloexec = (flags as u32) & InotifyFlags::CLOEXEC.bits() != 0;
    add_file_like(instance, cloexec).map(|fd| {
        debug!("sys_inotify_init1: allocated fd {fd}");
        fd as isize
    })
}

pub fn sys_inotify_add_watch(fd: i32, pathname: *const c_char, mask: u32) -> AxResult<isize> {
    debug!("inotify_add_watch called: fd={fd}, mask={mask:#x}");

    // Load pathname (using vm_load_string, same as sys_open)
    let path = vm_load_string(pathname)?;
    // Get file corresponding to file descriptor
    let file = get_file_like(fd)?;
    // Convert to inotify instance
    let inotify = match file.clone().into_any().downcast::<InotifyInstance>() {
        Ok(inst) => inst,
        Err(_) => {
            warn!("inotify_add_watch: fd {fd} is not an inotify instance");
            return Err(AxError::InvalidInput);
        }
    };

    // Add watch
    let wd = inotify.add_watch(&path, mask)?;

    info!("inotify watch added: fd={fd}, path={path}, wd={wd}");
    Ok(wd as isize)
}

pub fn sys_inotify_rm_watch(fd: i32, wd: i32) -> AxResult<isize> {
    debug!("sys_inotify_rm_watch: fd={fd}, wd={wd}");

    // Get file
    let file = get_file_like(fd)?;

    // Convert to inotify instance
    let inotify = match file.clone().into_any().downcast::<InotifyInstance>() {
        Ok(inst) => inst,
        Err(_) => {
            warn!("inotify_rm_watch: fd {fd} is not an inotify instance");
            return Err(AxError::InvalidInput);
        }
    };

    // Remove watch
    inotify.remove_watch(wd)?;
    info!("inotify_rm_watch: removed watch wd={wd} from fd={fd}");
    Ok(0)
}
