use axerrno::{AxError, AxResult};
use axhal::uspace::UserContext;
use starry_vm::VmPtr;

use super::clone::{CloneFlags, CloneParamProvider, do_clone};

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

impl CloneParamProvider for Clone3Args {
    fn flags(&self) -> CloneFlags {
        CloneFlags::from_bits_truncate(self.flags)
    }

    fn exit_signal(&self) -> u64 {
        self.exit_signal
    }

    fn stack_pointer(&self) -> usize {
        // For clone3(), stack + stack_size gives the SP
        if self.stack > 0 {
            if self.stack_size > 0 {
                // Stack grows downward, SP = base + size
                (self.stack + self.stack_size) as usize
            } else {
                // If only stack provided, treat as SP directly
                self.stack as usize
            }
        } else {
            0
        }
    }

    fn tls(&self) -> usize {
        self.tls as usize
    }

    fn child_settid_ptr(&self) -> usize {
        self.child_tid as usize
    }

    fn child_cleartid_ptr(&self) -> usize {
        self.child_tid as usize // for glibc compatibility
    }

    fn parent_tid_ptr(&self) -> usize {
        self.parent_tid as usize
    }

    fn pidfd_ptr(&self) -> usize {
        // For clone3(), pidfd is a separate field
        self.pidfd as usize
    }

    fn validate(&self) -> AxResult<()> {
        // Warn about unsupported features
        if self.set_tid != 0 || self.set_tid_size != 0 {
            warn!("sys_clone3: set_tid/set_tid_size not supported, ignoring");
        }
        if self.cgroup != 0 {
            warn!("sys_clone3: cgroup parameter not supported, ignoring");
        }

        // In clone3(), PIDFD and PARENT_SETTID can coexist
        // because they use separate fields (no validation needed)

        Ok(())
    }
}

pub fn sys_clone3(uctx: &UserContext, args_ptr: usize, args_size: usize) -> AxResult<isize> {
    debug!("sys_clone3 <= args_ptr: {args_ptr:#x}, args_size: {args_size}");

    // Validate size
    if args_size < MIN_CLONE_ARGS_SIZE {
        warn!("sys_clone3: args_size {args_size} too small, minimum is {MIN_CLONE_ARGS_SIZE}");
        return Err(AxError::InvalidInput);
    }

    if args_size > core::mem::size_of::<Clone3Args>() {
        debug!("sys_clone3: args_size {args_size} larger than expected, using known fields only");
    }

    // Copy arguments from user space
    let args_ptr = args_ptr as *const Clone3Args;
    let args = unsafe { args_ptr.vm_read_uninit()?.assume_init() };
    debug!("sys_clone3: args = {args:?}");

    // Use common implementation
    let result = do_clone(uctx, &args)?;
    debug!("sys_clone3 => child tid: {result}");

    Ok(result)
}
