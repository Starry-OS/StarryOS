use alloc::sync::Arc;

use axerrno::{AxError, AxResult};
use axfs::FS_CONTEXT;
use axhal::uspace::UserContext;
use axtask::{AxTaskExt, current, spawn_task};
use bitflags::bitflags;
use kspin::SpinNoIrq;
use linux_raw_sys::general::*;
use starry_core::{
    mm::copy_from_kernel,
    task::{AsThread, ProcessData, Thread, add_task_to_table},
};
use starry_process::Pid;
use starry_signal::Signo;

use crate::{
    file::{FD_TABLE, FileLike, PidFd},
    mm::UserPtr,
    task::new_user_task,
};

bitflags! {
    /// Options for use with [`sys_clone3`].
    #[derive(Debug, Clone, Copy, Default)]
    struct CloneFlags: u64 {
        /// The calling process and the child process run in the same
        /// memory space.
        const VM = CLONE_VM as u64;
        /// The caller and the child process share the same filesystem
        /// information.
        const FS = CLONE_FS as u64;
        /// The calling process and the child process share the same file
        /// descriptor table.
        const FILES = CLONE_FILES as u64;
        /// The calling process and the child process share the same table
        /// of signal handlers.
        const SIGHAND = CLONE_SIGHAND as u64;
        /// Sets pidfd to the child process's PID file descriptor.
        const PIDFD = CLONE_PIDFD as u64;
        /// If the calling process is being traced, then trace the child
        /// also.
        const PTRACE = CLONE_PTRACE as u64;
        /// The execution of the calling process is suspended until the
        /// child releases its virtual memory resources via a call to
        /// execve(2) or _exit(2) (as with vfork(2)).
        const VFORK = CLONE_VFORK as u64;
        /// The parent of the new child (as returned by getppid(2))
        /// will be the same as that of the calling process.
        const PARENT = CLONE_PARENT as u64;
        /// The child is placed in the same thread group as the calling
        /// process.
        const THREAD = CLONE_THREAD as u64;
        /// The cloned child is started in a new mount namespace.
        const NEWNS = CLONE_NEWNS as u64;
        /// The child and the calling process share a single list of System
        /// V semaphore adjustment values
        const SYSVSEM = CLONE_SYSVSEM as u64;
        /// The TLS (Thread Local Storage) descriptor is set to tls.
        const SETTLS = CLONE_SETTLS as u64;
        /// Store the child thread ID in the parent's memory.
        const PARENT_SETTID = CLONE_PARENT_SETTID as u64;
        /// Clear (zero) the child thread ID in child memory when the child
        /// exits, and do a wakeup on the futex at that address.
        const CHILD_CLEARTID = CLONE_CHILD_CLEARTID as u64;
        /// A tracing process cannot force `CLONE_PTRACE` on this child
        /// process.
        const UNTRACED = CLONE_UNTRACED as u64;
        /// Store the child thread ID in the child's memory.
        const CHILD_SETTID = CLONE_CHILD_SETTID as u64;
        /// Create the process in a new cgroup namespace.
        const NEWCGROUP = CLONE_NEWCGROUP as u64;
        /// Create the process in a new UTS namespace.
        const NEWUTS = CLONE_NEWUTS as u64;
        /// Create the process in a new IPC namespace.
        const NEWIPC = CLONE_NEWIPC as u64;
        /// Create the process in a new user namespace.
        const NEWUSER = CLONE_NEWUSER as u64;
        /// Create the process in a new PID namespace.
        const NEWPID = CLONE_NEWPID as u64;
        /// Create the process in a new network namespace.
        const NEWNET = CLONE_NEWNET as u64;
        /// The new process shares an I/O context with the calling process.
        const IO = CLONE_IO as u64;
        /// Clear signal handlers on clone (since Linux 5.5)
        const CLEAR_SIGHAND = 0x100000000u64;
        /// Clone into specific cgroup (since Linux 5.7)
        const INTO_CGROUP = 0x200000000u64;
    }
}

/// Structure passed to clone3() system call
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CloneArgs {
    /// Flags bit mask
    pub flags: u64,
    /// Where to store PID file descriptor (int *)
    pub pidfd: u64,
    /// Where to store child TID, in child's memory (pid_t *)
    pub child_tid: u64,
    /// Where to store child TID, in parent's memory (pid_t *)
    pub parent_tid: u64,
    /// Signal to deliver to parent on child termination
    pub exit_signal: u64,
    /// Pointer to lowest byte of stack
    pub stack: u64,
    /// Size of stack
    pub stack_size: u64,
    /// Location of new TLS
    pub tls: u64,
    /// Pointer to a pid_t array (since Linux 5.5)
    pub set_tid: u64,
    /// Number of elements in set_tid (since Linux 5.5)
    pub set_tid_size: u64,
    /// File descriptor for target cgroup of child (since Linux 5.7)
    pub cgroup: u64,
}

/// The minimum size of clone_args structure we support
const MIN_CLONE_ARGS_SIZE: usize = core::mem::size_of::<u64>() * 8; // First 8 fields

/// Validate clone_args structure and flags
fn validate_clone_args(args: &CloneArgs) -> AxResult<()> {
    let flags = CloneFlags::from_bits_truncate(args.flags);

    // Check for unsupported flag combinations
    if args.exit_signal > 0 && flags.contains(CloneFlags::THREAD | CloneFlags::PARENT) {
        return Err(AxError::InvalidInput);
    }

    // CLONE_THREAD requires CLONE_VM and CLONE_SIGHAND
    if flags.contains(CloneFlags::THREAD) && !flags.contains(CloneFlags::VM | CloneFlags::SIGHAND) {
        return Err(AxError::InvalidInput);
    }

    // Validate signal number
    if args.exit_signal > 0 && args.exit_signal >= 64 {
        return Err(AxError::InvalidInput);
    }

    // Validate set_tid_size
    if args.set_tid_size > 0 {
        warn!("sys_clone3: set_tid/set_tid_size not fully supported, ignoring");
        // In a full implementation, we would validate:
        // - set_tid_size <= nested PID namespace depth
        // - PIDs in set_tid array are available
    }

    // Validate cgroup fd
    if args.cgroup > 0 {
        warn!("sys_clone3: cgroup parameter not fully supported, ignoring");
    }

    // Namespace flags - stub support
    let namespace_flags = CloneFlags::NEWNS
        | CloneFlags::NEWIPC
        | CloneFlags::NEWNET
        | CloneFlags::NEWPID
        | CloneFlags::NEWUSER
        | CloneFlags::NEWUTS
        | CloneFlags::NEWCGROUP;

    if flags.intersects(namespace_flags) {
        warn!(
            "sys_clone3: namespace flags detected ({:?}), stub support only",
            flags & namespace_flags
        );
        // Don't return error, just log warning for compatibility
    }

    Ok(())
}

/// Implementation of clone3 system call
pub fn sys_clone3(uctx: &UserContext, args_ptr: usize, args_size: usize) -> AxResult<isize> {
    debug!(
        "sys_clone3 <= args_ptr: {:#x}, args_size: {}",
        args_ptr, args_size
    );

    // Validate arguments size
    if args_size < MIN_CLONE_ARGS_SIZE {
        warn!(
            "sys_clone3: args_size {} too small, minimum is {}",
            args_size, MIN_CLONE_ARGS_SIZE
        );
        return Err(AxError::InvalidInput);
    }

    // Support larger structures for forward compatibility
    if args_size > core::mem::size_of::<CloneArgs>() {
        // Just use what we understand, ignore extra fields
        debug!(
            "sys_clone3: args_size {} larger than expected {}, using known fields only",
            args_size,
            core::mem::size_of::<CloneArgs>()
        );
    }

    // Copy clone_args from user space
    let args_uptr = UserPtr::<CloneArgs>::from(args_ptr);
    let args = *args_uptr.get_as_mut()?;

    debug!("sys_clone3: args = {:?}", args);

    // Validate arguments
    validate_clone_args(&args)?;

    let mut flags = CloneFlags::from_bits_truncate(args.flags);

    // Handle VFORK special case (same as sys_clone)
    if flags.contains(CloneFlags::VFORK) {
        debug!("sys_clone3: CLONE_VFORK slow path");
        flags.remove(CloneFlags::VM);
    }

    debug!("sys_clone3: effective flags: {:?}", flags);

    // Parse exit signal
    let exit_signal = if args.exit_signal > 0 {
        Signo::from_repr(args.exit_signal as u8)
    } else {
        None
    };

    // Prepare new user context
    let mut new_uctx = *uctx;

    // Set stack pointer if provided
    if args.stack > 0 {
        if args.stack_size > 0 {
            // Stack grows downward, so set SP to stack + stack_size
            new_uctx.set_sp((args.stack + args.stack_size) as usize);
        } else {
            new_uctx.set_sp(args.stack as usize);
        }
    }

    // Set TLS if requested
    if flags.contains(CloneFlags::SETTLS) {
        new_uctx.set_tls(args.tls as usize);
    }

    // Child returns 0
    new_uctx.set_retval(0);

    // Prepare child_tid pointer if needed
    let set_child_tid = if flags.contains(CloneFlags::CHILD_SETTID) && args.child_tid > 0 {
        Some(UserPtr::<u32>::from(args.child_tid as usize).get_as_mut()?)
    } else {
        None
    };

    let curr = current();
    let old_proc_data = &curr.as_thread().proc_data;

    // Create new task
    let mut new_task = new_user_task(&curr.name(), new_uctx, set_child_tid);
    let tid = new_task.id().as_u64() as Pid;

    // Set parent_tid if requested
    if flags.contains(CloneFlags::PARENT_SETTID) && args.parent_tid > 0 {
        *UserPtr::<Pid>::from(args.parent_tid as usize).get_as_mut()? = tid;
    }

    // Create process data based on flags
    let new_proc_data = if flags.contains(CloneFlags::THREAD) {
        // Thread creation: share address space
        new_task
            .ctx_mut()
            .set_page_table_root(old_proc_data.aspace.lock().page_table_root());
        old_proc_data.clone()
    } else {
        // Process creation: fork or vfork
        let proc = if flags.contains(CloneFlags::PARENT) {
            old_proc_data.proc.parent().ok_or(AxError::InvalidInput)?
        } else {
            old_proc_data.proc.clone()
        }
        .fork(tid);

        // Handle address space
        let aspace = if flags.contains(CloneFlags::VM) {
            old_proc_data.aspace.clone()
        } else {
            let mut aspace = old_proc_data.aspace.lock();
            let aspace = aspace.try_clone()?;
            copy_from_kernel(&mut aspace.lock())?;
            aspace
        };

        new_task
            .ctx_mut()
            .set_page_table_root(aspace.lock().page_table_root());

        // Handle signal handlers
        let signal_actions = if flags.contains(CloneFlags::SIGHAND) {
            old_proc_data.signal.actions.clone()
        } else if flags.contains(CloneFlags::CLEAR_SIGHAND) {
            // CLONE_CLEAR_SIGHAND: reset to default handlers
            Arc::new(SpinNoIrq::new(Default::default()))
        } else {
            // Normal fork: copy signal handlers
            Arc::new(SpinNoIrq::new(old_proc_data.signal.actions.lock().clone()))
        };

        let proc_data = ProcessData::new(
            proc,
            old_proc_data.exe_path.read().clone(),
            old_proc_data.cmdline.read().clone(),
            aspace,
            signal_actions,
            exit_signal,
        );
        proc_data.set_umask(old_proc_data.umask());

        // Handle file descriptors and filesystem context
        {
            let mut scope = proc_data.scope.write();

            if flags.contains(CloneFlags::FILES) {
                // Share file descriptor table
                FD_TABLE.scope_mut(&mut scope).clone_from(&FD_TABLE);
            } else {
                // Copy file descriptor table
                FD_TABLE
                    .scope_mut(&mut scope)
                    .write()
                    .clone_from(&FD_TABLE.read());
            }

            if flags.contains(CloneFlags::FS) {
                // Share filesystem context
                FS_CONTEXT.scope_mut(&mut scope).clone_from(&FS_CONTEXT);
            } else {
                // Copy filesystem context
                FS_CONTEXT
                    .scope_mut(&mut scope)
                    .lock()
                    .clone_from(&FS_CONTEXT.lock());
            }
        }

        proc_data
    };

    // Add thread to process
    new_proc_data.proc.add_thread(tid);

    // Handle PIDFD if requested
    if flags.contains(CloneFlags::PIDFD) && args.pidfd > 0 {
        let pidfd = PidFd::new(&new_proc_data);
        let fd = pidfd.add_to_fd_table(true)?;
        *UserPtr::<i32>::from(args.pidfd as usize).get_as_mut()? = fd;
    }

    // Create thread object
    let thr = Thread::new(tid, new_proc_data);

    // Set clear_child_tid if requested
    if flags.contains(CloneFlags::CHILD_CLEARTID) && args.child_tid > 0 {
        thr.set_clear_child_tid(args.child_tid as usize);
    }

    *new_task.task_ext_mut() = Some(unsafe { AxTaskExt::from_impl(thr) });

    // Spawn the task
    let task = spawn_task(new_task);
    add_task_to_table(&task);

    debug!("sys_clone3 => child tid: {}", tid);

    Ok(tid as _)
}
