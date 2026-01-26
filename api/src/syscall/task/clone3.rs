use axerrno::{AxError, AxResult};
use axhal::uspace::UserContext;
use starry_vm::VmPtr;

use super::clone::{CloneArgs, CloneFlags, do_clone};

/// Structure passed to clone3() system call.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Clone3Args {
    pub flags: u64,
    pub pidfd: u64,
    pub child_tid: u64,
    pub parent_tid: u64,
    pub exit_signal: u64,
    pub stack: u64,
    pub stack_size: u64,
    pub tls: u64,
    pub set_tid: u64,
    pub set_tid_size: u64,
    pub cgroup: u64,
}

const MIN_CLONE_ARGS_SIZE: usize = core::mem::size_of::<u64>() * 8;

impl Clone3Args {
    fn into_clone_args(self) -> AxResult<CloneArgs> {
        if self.set_tid != 0 || self.set_tid_size != 0 {
            warn!("sys_clone3: set_tid/set_tid_size not supported, ignoring");
        }
        if self.cgroup != 0 {
            warn!("sys_clone3: cgroup parameter not supported, ignoring");
        }

        let flags = CloneFlags::from_bits_truncate(self.flags);

        let stack = if self.stack > 0 {
            if self.stack_size > 0 {
                (self.stack + self.stack_size) as usize
            } else {
                self.stack as usize
            }
        } else {
            0
        };

        Ok(CloneArgs {
            flags,
            exit_signal: self.exit_signal,
            stack,
            tls: self.tls as usize,
            parent_tid: self.parent_tid as usize,
            child_tid: self.child_tid as usize,
            pidfd: self.pidfd as usize,
        })
    }
}

pub fn sys_clone3(uctx: &UserContext, args_ptr: usize, args_size: usize) -> AxResult<isize> {
    debug!("sys_clone3 <= args_ptr: {args_ptr:#x}, args_size: {args_size}");

    if args_size < MIN_CLONE_ARGS_SIZE {
        warn!("sys_clone3: args_size {args_size} too small, minimum is {MIN_CLONE_ARGS_SIZE}");
        return Err(AxError::InvalidInput);
    }

    if args_size > core::mem::size_of::<Clone3Args>() {
        debug!("sys_clone3: args_size {args_size} larger than expected, using known fields only");
    }

    let args_ptr = args_ptr as *const Clone3Args;
    let clone3_args = unsafe { args_ptr.vm_read_uninit()?.assume_init() };

    debug!("sys_clone3: args = {clone3_args:?}");

    let args = clone3_args.into_clone_args()?;
    do_clone(uctx, args)
}
