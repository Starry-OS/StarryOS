use core::ffi::c_char;

use axerrno::{AxError, AxResult};

use crate::{
    file::{
        add_file_like, get_file_like,
        inotify::{InotifyFlags, InotifyInstance},
    },
    mm::{UserPtr, vm_load_string},
};
pub fn sys_inotify_init1(flags: i32) -> AxResult<isize> {
    let instance = InotifyInstance::new(flags)?;

    let flags_u32 = flags as u32;

    // Validate flags - only allow CLOEXEC and NONBLOCK
    let inotify_flags = InotifyFlags::from_bits(flags_u32).ok_or_else(|| {
        warn!("sys_inotify_init1: invalid flags {:#x}", flags);
        AxError::InvalidInput
    })?;

    // Use add_file_like to add file descriptor
    let cloexec = inotify_flags.contains(InotifyFlags::CLOEXEC);
    add_file_like(instance, cloexec).map(|fd| {
        debug!("sys_inotify_init1: allocated fd {}", fd);
        fd as isize
    })
}

pub fn sys_inotify_add_watch(fd: i32, pathname: UserPtr<u8>, mask: u32) -> AxResult<isize> {
    debug!("inotify_add_watch called: fd={}, mask={:#x}", fd, mask);
    // Validate mask
    if mask == 0 {
        warn!("inotify_add_watch: mask contains no valid events");
        return Err(AxError::InvalidInput);
    }

    // Load pathname (using vm_load_string, same as sys_open)
    // Get raw pointer from UserPtr
    let addr = pathname.address();
    let ptr = addr.as_ptr() as *const c_char;
    let path = vm_load_string(ptr)?;
    // Get file corresponding to file descriptor
    let file = get_file_like(fd)?;
    // Convert to inotify instance
    let inotify = match file.clone().into_any().downcast::<InotifyInstance>() {
        Ok(inst) => inst,
        Err(_) => {
            warn!("inotify_add_watch: fd {} is not an inotify instance", fd);
            return Err(AxError::InvalidInput);
        }
    };
    // Get current process information (for permission checks)
    // let curr = current();
    // let proc_data = &curr.as_thread().proc_data;

    // Add watch
    let wd = inotify.add_watch(&path, mask)?;

    info!("inotify watch added: fd={}, path={}, wd={}", fd, path, wd);
    Ok(wd as isize)
}

pub fn sys_inotify_rm_watch(fd: i32, wd: i32) -> AxResult<isize> {
    debug!("sys_inotify_rm_watch: fd={}, wd={}", fd, wd);

    // Get file
    let file = match get_file_like(fd) {
        Ok(f) => f,
        Err(AxError::BadFileDescriptor) => {
            warn!("inotify_rm_watch: bad file descriptor {}", fd);
            return Err(AxError::BadFileDescriptor);
        }
        Err(e) => {
            warn!("inotify_rm_watch: get_file_like failed: {:?}", e);
            return Err(AxError::InvalidInput);
        }
    };

    // Convert to inotify instance
    let inotify = match file.clone().into_any().downcast::<InotifyInstance>() {
        Ok(inst) => inst,
        Err(_) => {
            warn!("inotify_rm_watch: fd {} is not an inotify instance", fd);
            return Err(AxError::InvalidInput);
        }
    };

    // Remove watch
    match inotify.remove_watch(wd) {
        Ok(()) => {
            info!("inotify_rm_watch: removed watch wd={} from fd={}", wd, fd);
            Ok(0)
        }
        Err(e) => {
            warn!("inotify_rm_watch: remove_watch failed: {:?}", e);
            // TODO
            // It would be better to create a watch error for inotify
            // Error conversion
            Err(AxError::InvalidInput)
        }
    }
}
