use axerrno::{AxError, AxResult};
use axhal::uspace::UserContext;
use starry_vm::VmPtr;

use super::clone::{CloneArgs, CloneFlags};

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

// SAFETY: Clone3Args is a POD type with all fields being u64, which are Zeroable
unsafe impl bytemuck::Zeroable for Clone3Args {}

// SAFETY: Clone3Args is a POD type with no invalid bit patterns
unsafe impl bytemuck::AnyBitPattern for Clone3Args {}

const MIN_CLONE_ARGS_SIZE: usize = core::mem::size_of::<u64>() * 8;

impl TryFrom<Clone3Args> for CloneArgs {
    type Error = axerrno::AxError;

    fn try_from(args: Clone3Args) -> AxResult<Self> {
        if args.set_tid != 0 || args.set_tid_size != 0 {
            warn!("sys_clone3: set_tid/set_tid_size not supported, ignoring");
        }
        if args.cgroup != 0 {
            warn!("sys_clone3: cgroup parameter not supported, ignoring");
        }

        let flags = CloneFlags::from_bits_truncate(args.flags);

        let stack = if args.stack > 0 {
            if args.stack_size > 0 {
                (args.stack + args.stack_size) as usize
            } else {
                args.stack as usize
            }
        } else {
            0
        };

        Ok(CloneArgs {
            flags,
            exit_signal: args.exit_signal,
            stack,
            tls: args.tls as usize,
            parent_tid: args.parent_tid as usize,
            child_tid: args.child_tid as usize,
            pidfd: args.pidfd as usize,
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
    let clone3_args = args_ptr.vm_read()?;

    debug!("sys_clone3: args = {clone3_args:?}");

    let args = CloneArgs::try_from(clone3_args)?;
    args.do_clone(uctx)
}