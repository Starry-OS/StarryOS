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
use starry_vm::VmMutPtr;

use crate::{
    file::{FD_TABLE, FileLike, PidFd},
    task::new_user_task,
};

bitflags! {
    /// Clone flags for process/thread creation.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct CloneFlags: u64 {
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

/// Trait for providing clone parameters in a flexible way.
///
/// This allows clone() and clone3() to have different parameter semantics
/// while sharing the core implementation logic.
pub trait CloneParamProvider {
    /// Get clone flags
    fn flags(&self) -> CloneFlags;

    /// Get exit signal (0 means no signal)
    fn exit_signal(&self) -> u64;

    /// Get new stack pointer (0 means inherit parent's)
    fn stack_pointer(&self) -> usize;

    /// Get TLS value
    fn tls(&self) -> usize;

    /// Get child_tid pointer for CHILD_SETTID
    fn child_settid_ptr(&self) -> usize;
    fn child_cleartid_ptr(&self) -> usize;

    /// Get parent_tid pointer for PARENT_SETTID (used by both clone and clone3)
    fn parent_tid_ptr(&self) -> usize;

    /// Get pidfd pointer (0 if not used)
    /// - For clone(): returns 0 (uses parent_tid_ptr instead)
    /// - For clone3(): returns the pidfd field
    fn pidfd_ptr(&self) -> usize;

    /// Validate parameters (different rules for clone vs clone3)
    fn validate(&self) -> AxResult<()>;
}

/// Common validation logic shared by all clone variants
fn validate_common(flags: CloneFlags, exit_signal: u64) -> AxResult<()> {
    // Check for invalid flag combinations
    // The original logic is retained here for the time being.
    // In the future, it can be ignored and set to 0 simultaneously without reporting an error in some cases.
    if exit_signal > 0 && flags.contains(CloneFlags::THREAD | CloneFlags::PARENT) {
        return Err(AxError::InvalidInput);
    }

    if flags.contains(CloneFlags::THREAD) && !flags.contains(CloneFlags::VM | CloneFlags::SIGHAND) {
        return Err(AxError::InvalidInput);
    }

    // https://man7.org/linux/man-pages/man2/clone.2.html
    // CLONE_SIGHAND
    // Since Linux 2.6.0, the flags mask must also include CLONE_VM if CLONE_SIGHAND is specified.
    if flags.contains(CloneFlags::SIGHAND) && !flags.contains(CloneFlags::VM) {
        return Err(AxError::InvalidInput);
    }

    if flags.contains(CloneFlags::VFORK) && flags.contains(CloneFlags::THREAD) {
        return Err(AxError::InvalidInput);
    }

    // Validate exit signal range
    if exit_signal >= 64 {
        return Err(AxError::InvalidInput);
    }

    // Namespace flags warning
    let namespace_flags = CloneFlags::NEWNS
        | CloneFlags::NEWIPC
        | CloneFlags::NEWNET
        | CloneFlags::NEWPID
        | CloneFlags::NEWUSER
        | CloneFlags::NEWUTS
        | CloneFlags::NEWCGROUP;

    if flags.intersects(namespace_flags) {
        warn!(
            "sys_clone/sys_clone3: namespace flags detected ({:?}), stub support only",
            flags & namespace_flags
        );
    }

    Ok(())
}

/// Core implementation of clone/clone3/fork/vfork.
///
/// This function contains the shared logic for creating new tasks.
/// Different parameter semantics are handled through the `CloneParamProvider` trait.
pub fn do_clone<P: CloneParamProvider>(uctx: &UserContext, params: &P) -> AxResult<isize> {
    // Validate parameters
    params.validate()?;

    let mut flags = params.flags();
    let exit_signal = params.exit_signal();

    // Common validation
    validate_common(flags, exit_signal)?;

    // Handle VFORK special case
    // NOTE:
    // CLONE_VFORK currently shares address space,
    // but does NOT suspend parent execution.
    // This is a partial implementation.
    if flags.contains(CloneFlags::VFORK) {
        debug!("do_clone: CLONE_VFORK slow path");
        flags.remove(CloneFlags::VM);
    }

    debug!(
        "do_clone: flags={flags:?}, exit_signal={exit_signal}, stack={:#x}, tls={:#x}",
        params.stack_pointer(),
        params.tls()
    );

    let exit_signal = if exit_signal > 0 {
        Signo::from_repr(exit_signal as u8)
    } else {
        None
    };

    // Prepare new user context
    let mut new_uctx = *uctx;
    let stack_ptr = params.stack_pointer();
    if stack_ptr != 0 {
        new_uctx.set_sp(stack_ptr);
    }
    if flags.contains(CloneFlags::SETTLS) {
        new_uctx.set_tls(params.tls());
    }
    new_uctx.set_retval(0);

    let set_child_tid = if flags.contains(CloneFlags::CHILD_SETTID) {
        child_tid
    } else {
        0
    };

    let curr = current();
    let old_proc_data = &curr.as_thread().proc_data;

    // Create new task
    let mut new_task = new_user_task(&curr.name(), new_uctx, set_child_tid);
    let tid = new_task.id().as_u64() as Pid;
    if flags.contains(CloneFlags::PARENT_SETTID) {
        (parent_tid as *mut Pid).vm_write(tid).ok();

    // Write parent TID if PARENT_SETTID is set
    let parent_tid_ptr = params.parent_tid_ptr();
    if flags.contains(CloneFlags::PARENT_SETTID) && parent_tid_ptr != 0 {
        *UserPtr::<Pid>::from(parent_tid_ptr).get_as_mut()? = tid;
    }

    // Create process data based on flags (keep original inline logic)
    let new_proc_data = if flags.contains(CloneFlags::THREAD) {
        // Thread creation: share address space
        new_task
            .ctx_mut()
            .set_page_table_root(old_proc_data.aspace.lock().page_table_root());
        old_proc_data.clone()
    } else {
        // Process creation
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
        // Inherit heap pointers from parent to ensure child's heap state is consistent after fork
        proc_data.set_heap_top(old_proc_data.get_heap_top());

        // Handle file descriptors and filesystem context
        {
            let mut scope = proc_data.scope.write();

            if flags.contains(CloneFlags::FILES) {
                FD_TABLE.scope_mut(&mut scope).clone_from(&FD_TABLE);
            } else {
                FD_TABLE
                    .scope_mut(&mut scope)
                    .write()
                    .clone_from(&FD_TABLE.read());
            }

            if flags.contains(CloneFlags::FS) {
                FS_CONTEXT.scope_mut(&mut scope).clone_from(&FS_CONTEXT);
            } else {
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
    // Different behavior for clone() vs clone3()
    if flags.contains(CloneFlags::PIDFD) {
        let pidfd = PidFd::new(&new_proc_data);
        (parent_tid as *mut i32).vm_write(pidfd.add_to_fd_table(true)?)?;
        let fd = pidfd.add_to_fd_table(true)?;

        // Get the correct pointer based on clone variant
        let pidfd_target_ptr = params.pidfd_ptr();
        if pidfd_target_ptr != 0 {
            // clone3: write to pidfd field
            *UserPtr::<i32>::from(pidfd_target_ptr).get_as_mut()? = fd;
        } else if parent_tid_ptr != 0 {
            // clone: write to parent_tid (historical behavior)
            *UserPtr::<i32>::from(parent_tid_ptr).get_as_mut()? = fd;
        }
    }

    // Create thread object
    let thr = Thread::new(tid, new_proc_data);

    // Set clear_child_tid if requested
    let clear_child_tid_ptr = params.child_cleartid_ptr();
    if flags.contains(CloneFlags::CHILD_CLEARTID) && clear_child_tid_ptr != 0 {
        thr.set_clear_child_tid(clear_child_tid_ptr);
    }

    *new_task.task_ext_mut() = Some(unsafe { AxTaskExt::from_impl(thr) });

    // Spawn the task
    let task = spawn_task(new_task);
    add_task_to_table(&task);

    Ok(tid as _)
}

// ================================
// Clone (legacy) parameters
// ================================

/// Parameters for the clone() system call.
///
/// Note: In clone(), the parent_tid parameter serves dual purpose:
/// - If CLONE_PIDFD: receives the pidfd
/// - If CLONE_PARENT_SETTID: receives the child TID
///   These two flags are mutually exclusive in clone().
pub struct CloneParams {
    flags: u32,
    stack: usize,
    parent_tid: usize,
    child_tid: usize,
    tls: usize,
}

impl CloneParams {
    pub fn new(flags: u32, stack: usize, parent_tid: usize, child_tid: usize, tls: usize) -> Self {
        Self {
            flags,
            stack,
            parent_tid,
            child_tid,
            tls,
        }
    }
}

impl CloneParamProvider for CloneParams {
    fn flags(&self) -> CloneFlags {
        const FLAG_MASK: u32 = 0xff;
        CloneFlags::from_bits_truncate((self.flags & !FLAG_MASK) as u64)
    }

    fn exit_signal(&self) -> u64 {
        const FLAG_MASK: u32 = 0xff;
        (self.flags & FLAG_MASK) as u64
    }

    fn stack_pointer(&self) -> usize {
        // For clone(), stack directly specifies the new SP
        self.stack
    }

    fn tls(&self) -> usize {
        self.tls
    }

    fn child_settid_ptr(&self) -> usize {
        self.child_tid
    }

    fn child_cleartid_ptr(&self) -> usize {
        self.child_tid
    }

    fn parent_tid_ptr(&self) -> usize {
        self.parent_tid
    }

    fn pidfd_ptr(&self) -> usize {
        // For clone(), PIDFD uses parent_tid, so return 0 here
        // The core logic will use parent_tid_ptr() instead
        0
    }

    fn validate(&self) -> AxResult<()> {
        let flags = self.flags();

        // In clone(), PIDFD and PARENT_SETTID are mutually exclusive
        // because they share the parent_tid parameter
        if flags.contains(CloneFlags::PIDFD) && flags.contains(CloneFlags::PARENT_SETTID) {
            return Err(AxError::InvalidInput);
        }

        Ok(())
    }
}

// ================================
// System call wrappers
// ================================

pub fn sys_clone(
    uctx: &UserContext,
    flags: u32,
    stack: usize,
    parent_tid: usize,
    #[cfg(any(target_arch = "x86_64", target_arch = "loongarch64"))] child_tid: usize,
    tls: usize,
    #[cfg(not(any(target_arch = "x86_64", target_arch = "loongarch64")))] child_tid: usize,
) -> AxResult<isize> {
    let params = CloneParams::new(flags, stack, parent_tid, child_tid, tls);
    do_clone(uctx, &params)
}

#[cfg(target_arch = "x86_64")]
pub fn sys_fork(uctx: &UserContext) -> AxResult<isize> {
    sys_clone(uctx, SIGCHLD, 0, 0, 0, 0)
}
