use alloc::vec::Vec;
use core::{fmt, time::Duration};

use axerrno::{AxError, AxResult};
use axpoll::IoEvents;
use axtask::future::{self, block_on, poll_io};
use bitmaps::Bitmap;
use linux_raw_sys::{
    general::*,
    select_macros::{FD_ISSET, FD_SET, FD_ZERO},
};
use starry_signal::SignalSet;
use starry_vm::{VmMutPtr, VmPtr};

use super::FdPollSet;
use crate::{
    file::FD_TABLE, signal::with_replacen_blocked, syscall::signal::check_sigset_size,
    time::TimeValueLike,
};

struct FdSet(Bitmap<{ __FD_SETSIZE as usize }>);

impl FdSet {
    fn new(nfds: usize, fds: Option<&__kernel_fd_set>) -> Self {
        let mut bitmap = Bitmap::new();
        if let Some(fds) = fds {
            for i in 0..nfds {
                if unsafe { FD_ISSET(i as _, fds) } {
                    bitmap.set(i, true);
                }
            }
        }
        Self(bitmap)
    }
}

impl fmt::Debug for FdSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.0).finish()
    }
}

fn do_select(
    nfds: u32,
    readfds: *mut __kernel_fd_set,
    writefds: *mut __kernel_fd_set,
    exceptfds: *mut __kernel_fd_set,
    timeout: Option<Duration>,
    sigmask: *const SignalSetWithSize,
) -> AxResult<isize> {
    if nfds > __FD_SETSIZE {
        return Err(AxError::InvalidInput);
    }
    let sigmask = match sigmask.nullable() {
        Some(p) => {
            let sig = unsafe { p.vm_read_uninit()?.assume_init() };
            check_sigset_size(sig.sigsetsize)?;
            match sig.set.nullable() {
                Some(sp) => Some(unsafe { sp.vm_read_uninit()?.assume_init() }),
                None => None,
            }
        }
        None => None,
    };

    let mut read_local = match readfds.nullable() {
        Some(p) => Some(unsafe { p.vm_read_uninit()?.assume_init() }),
        None => None,
    };

    let mut write_local = match writefds.nullable() {
        Some(p) => Some(unsafe { p.vm_read_uninit()?.assume_init() }),
        None => None,
    };
    let mut except_local = match exceptfds.nullable() {
        Some(p) => Some(unsafe { p.vm_read_uninit()?.assume_init() }),
        None => None,
    };

    let read_set = FdSet::new(nfds as _, read_local.as_ref());
    let write_set = FdSet::new(nfds as _, write_local.as_ref());
    let except_set = FdSet::new(nfds as _, except_local.as_ref());

    debug!(
        "sys_select <= nfds: {nfds} sets: [read: {read_set:?}, write: {write_set:?}, except: \
         {except_set:?}] timeout: {timeout:?}"
    );

    let fd_table = FD_TABLE.read();
    let fd_bitmap = read_set.0 | write_set.0 | except_set.0;
    let fd_count = fd_bitmap.len();
    let mut fds = Vec::with_capacity(fd_count);
    let mut fd_indices = Vec::with_capacity(fd_count);
    for fd in fd_bitmap.into_iter() {
        let f = fd_table
            .get(fd)
            .ok_or(AxError::BadFileDescriptor)?
            .inner
            .clone();
        let mut events = IoEvents::empty();
        events.set(IoEvents::IN, read_set.0.get(fd));
        events.set(IoEvents::OUT, write_set.0.get(fd));
        events.set(IoEvents::ERR, except_set.0.get(fd));
        if !events.is_empty() {
            fds.push((f, events));
            fd_indices.push(fd);
        }
    }

    drop(fd_table);
    let fds = FdPollSet(fds);

    if let Some(read_local) = read_local.as_mut() {
        unsafe { FD_ZERO(read_local) };
    }
    if let Some(write_local) = write_local.as_mut() {
        unsafe { FD_ZERO(write_local) };
    }
    if let Some(except_local) = except_local.as_mut() {
        unsafe { FD_ZERO(except_local) };
    }
    with_replacen_blocked(sigmask, || {
        match block_on(future::timeout(
            timeout,
            poll_io(&fds, IoEvents::empty(), false, || {
                let mut res = 0usize;
                for ((fd, interested), index) in fds.0.iter().zip(fd_indices.iter().copied()) {
                    let events = fd.poll() & *interested;
                    if events.contains(IoEvents::IN)
                        && let Some(set) = read_local.as_mut()
                    {
                        res += 1;
                        unsafe { FD_SET(index as _, set) };
                    }
                    if events.contains(IoEvents::OUT)
                        && let Some(set) = write_local.as_mut()
                    {
                        res += 1;
                        unsafe { FD_SET(index as _, set) };
                    }
                    if events.contains(IoEvents::ERR)
                        && let Some(set) = except_local.as_mut()
                    {
                        res += 1;
                        unsafe { FD_SET(index as _, set) };
                    }
                }
                if res > 0 {
                    return Ok(res as _);
                }

                Err(AxError::WouldBlock)
            }),
        )) {
            Ok(r) => r,
            Err(_) => Ok(0),
        }
    })
    .and_then(|ret| {
        if let Some((p, set)) = readfds.nullable().zip(read_local) {
            p.vm_write(set)?;
        }
        if let Some((p, set)) = writefds.nullable().zip(write_local) {
            p.vm_write(set)?;
        }
        if let Some((p, set)) = exceptfds.nullable().zip(except_local) {
            p.vm_write(set)?;
        }
        Ok(ret)
    })
}

#[cfg(target_arch = "x86_64")]
pub fn sys_select(
    nfds: u32,
    readfds: *mut __kernel_fd_set,
    writefds: *mut __kernel_fd_set,
    exceptfds: *mut __kernel_fd_set,
    timeout: *const timeval,
) -> AxResult<isize> {
    do_select(
        nfds,
        readfds,
        writefds,
        exceptfds,
        timeout
            .nullable()
            .map(|p| unsafe { p.vm_read_uninit()?.assume_init().try_into_time_value() })
            .transpose()?,
        core::ptr::null(),
    )
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SignalSetWithSize {
    set: *const SignalSet,
    sigsetsize: usize,
}

pub fn sys_pselect6(
    nfds: u32,
    readfds: *mut __kernel_fd_set,
    writefds: *mut __kernel_fd_set,
    exceptfds: *mut __kernel_fd_set,
    timeout: *const timespec,
    sigmask: *const SignalSetWithSize,
) -> AxResult<isize> {
    do_select(
        nfds,
        readfds,
        writefds,
        exceptfds,
        timeout
            .nullable()
            .map(|p| unsafe { p.vm_read_uninit()?.assume_init().try_into_time_value() })
            .transpose()?,
        sigmask,
    )
}
