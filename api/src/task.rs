use core::{ffi::c_long, future::poll_fn, sync::atomic::Ordering, task::Poll};

use axerrno::{AxError, AxResult};
use axhal::uspace::{ExceptionKind, ReturnReason, UserContext};
use axtask::{CurrentTask, TaskInner, current, future::block_on};
use bytemuck::AnyBitPattern;
use linux_raw_sys::general::ROBUST_LIST_LIMIT;
use starry_core::{
    futex::FutexKey,
    mm::access_user_memory,
    shm::SHM_MANAGER,
    task::{
        AsThread, Thread, get_process_data, get_task, send_signal_to_process,
        send_signal_to_thread, set_timer_state,
    },
    time::TimerState,
};
use starry_process::Pid;
use starry_signal::{SignalInfo, Signo};
use starry_vm::{VmMutPtr, VmPtr};

use crate::{
    signal::{check_signals, unblock_next_signal},
    syscall::handle_syscall,
};

/// Create a new user task.
pub fn new_user_task(
    name: &str,
    mut uctx: UserContext,
    set_child_tid: Option<&'static mut Pid>,
) -> TaskInner {
    TaskInner::new(
        move || {
            let curr = axtask::current();
            access_user_memory(|| {
                if let Some(tid) = set_child_tid {
                    *tid = curr.id().as_u64() as Pid;
                }
            });

            info!("Enter user space: ip={:#x}, sp={:#x}", uctx.ip(), uctx.sp());

            let thr = curr.as_thread();
            while !thr.pending_exit() {
                let reason = uctx.run();

                set_timer_state(&curr, TimerState::Kernel);

                match reason {
                    ReturnReason::Syscall => handle_syscall(&mut uctx),
                    ReturnReason::PageFault(addr, flags) => {
                        if !thr.proc_data.aspace.lock().handle_page_fault(addr, flags) {
                            info!(
                                "{:?}: segmentation fault at {:#x} {:?}",
                                thr.proc_data.proc, addr, flags
                            );
                            raise_signal_fatal(SignalInfo::new_kernel(Signo::SIGSEGV))
                                .expect("Failed to send SIGSEGV");
                        }
                    }
                    ReturnReason::Interrupt => {}
                    #[allow(unused_labels)]
                    ReturnReason::Exception(exc_info) => 'exc: {
                        // TODO: detailed handling
                        let signo = match exc_info.kind() {
                            ExceptionKind::Misaligned => {
                                #[cfg(target_arch = "loongarch64")]
                                if unsafe { uctx.emulate_unaligned() }.is_ok() {
                                    break 'exc;
                                }
                                Signo::SIGBUS
                            }
                            ExceptionKind::Breakpoint => Signo::SIGTRAP,
                            ExceptionKind::IllegalInstruction => Signo::SIGILL,
                            _ => Signo::SIGTRAP,
                        };
                        raise_signal_fatal(SignalInfo::new_kernel(signo))
                            .expect("Failed to send SIGTRAP");
                    }
                    r => {
                        warn!("Unexpected return reason: {r:?}");
                        raise_signal_fatal(SignalInfo::new_kernel(Signo::SIGSEGV))
                            .expect("Failed to send SIGSEGV");
                    }
                }

                // Check for a potential stop signal
                if !unblock_next_signal() {
                    while check_signals(thr, &mut uctx, None) {}
                }

                // Handle the stop signal if any
                handle_stopped_state(&curr, thr);

                // Check for potential continue or kill signal
                // after a `Ready` result is returned from the helper
                // function handling the potential stopped state.
                // This time, the state of the process may be
                // set to be `Running` if it was previously stopped
                // in the `do_continue` function within `check_signals`.
                if !unblock_next_signal() {
                    while check_signals(thr, &mut uctx, None) {}
                }

                set_timer_state(&curr, TimerState::User);
                // Clear interrupt state
                let _ = curr.interrupted();
            }
        },
        name.into(),
        starry_core::config::KERNEL_STACK_SIZE,
    )
}

#[repr(C)]
#[derive(Debug, Copy, Clone, AnyBitPattern)]
pub struct RobustList {
    pub next: *mut RobustList,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, AnyBitPattern)]
pub struct RobustListHead {
    pub list: RobustList,
    pub futex_offset: c_long,
    pub list_op_pending: *mut RobustList,
}

fn handle_futex_death(entry: *mut RobustList, offset: i64) -> AxResult<()> {
    let address = (entry as u64)
        .checked_add_signed(offset)
        .ok_or(AxError::InvalidInput)?;
    let address: usize = address.try_into().map_err(|_| AxError::InvalidInput)?;
    let key = FutexKey::new_current(address);

    let curr = current();
    let futex_table = curr.as_thread().proc_data.futex_table_for(&key);

    let Some(futex) = futex_table.get(&key) else {
        return Ok(());
    };
    futex.owner_dead.store(true, Ordering::SeqCst);
    futex.wq.wake(1, u32::MAX);
    Ok(())
}

pub fn exit_robust_list(head: *const RobustListHead) -> AxResult<()> {
    // Reference: https://elixir.bootlin.com/linux/v6.13.6/source/kernel/futex/core.c#L777

    let mut limit = ROBUST_LIST_LIMIT;

    let end_ptr = unsafe { &raw const (*head).list };
    let head = head.vm_read()?;
    let mut entry = head.list.next;
    let offset = head.futex_offset;
    let pending = head.list_op_pending;

    while !core::ptr::eq(entry, end_ptr) {
        let next_entry = entry.vm_read()?.next;
        if entry != pending {
            handle_futex_death(entry, offset)?;
        }
        entry = next_entry;

        limit -= 1;
        if limit == 0 {
            return Err(AxError::FilesystemLoop);
        }
        axtask::yield_now();
    }

    Ok(())
}

pub fn do_exit(exit_code: i32, group_exit: bool) {
    let curr = current();
    let thr = curr.as_thread();

    info!("{} exit with code: {}", curr.id_name(), exit_code);

    let clear_child_tid = thr.clear_child_tid() as *mut u32;
    if clear_child_tid.vm_write(0).is_ok() {
        let key = FutexKey::new_current(clear_child_tid as usize);
        let table = thr.proc_data.futex_table_for(&key);
        let guard = table.get(&key);
        if let Some(futex) = guard {
            futex.wq.wake(1, u32::MAX);
        }
        axtask::yield_now();
    }
    let head = thr.robust_list_head() as *const RobustListHead;
    if !head.is_null()
        && let Err(err) = exit_robust_list(head)
    {
        warn!("exit robust list failed: {err:?}");
    }

    let process = &thr.proc_data.proc;
    if process.exit_thread(curr.id().as_u64() as Pid, exit_code) {
        // Transition the process to ZOMBIE state before reparenting children.
        //
        // Rationale for this ordering:
        // 1. Ensures the parent's wait() sees the `ZOMBIE` state immediately upon
        //    wakeup (via Release-Acquire memory ordering with `is_zombie()`).
        // 2. Maintains single responsibility: `Process::exit()` only handles child
        //    reparenting, while state transitions remain explicit at the API layer.
        process.transition_to_zombie();
        process.exit();
        if let Some(parent) = process.parent() {
            if let Some(signo) = thr.proc_data.exit_signal {
                let _ = send_signal_to_process(parent.pid(), Some(SignalInfo::new_kernel(signo)));
            }
            if let Ok(data) = get_process_data(parent.pid()) {
                data.child_exit_event.wake();
            }
        }
        thr.proc_data.exit_event.wake();

        SHM_MANAGER.lock().clear_proc_shm(process.pid());
    }
    if group_exit && !process.is_group_exited() {
        process.group_exit();
        let sig = SignalInfo::new_kernel(Signo::SIGKILL);
        for tid in process.threads() {
            let _ = send_signal_to_thread(None, tid, Some(sig.clone()));
        }
    }
    thr.set_exit();
}

/// Sends a fatal signal to the current process.
pub fn raise_signal_fatal(sig: SignalInfo) -> AxResult<()> {
    let curr = current();
    let proc_data = &curr.as_thread().proc_data;

    let signo = sig.signo();
    info!("Send fatal signal {signo:?} to the current process");
    if let Some(tid) = proc_data.signal.send_signal(sig)
        && let Ok(task) = get_task(tid)
    {
        task.interrupt();
    } else {
        // No task wants to handle the signal, abort the task
        do_exit(signo as i32, true);
    }

    Ok(())
}

/// Handle the potential stopped state of the current process,
/// actually performing the stoppage.
///
/// The procedure is as follows:
/// 1. Check if the process is stopped. If not, return immediately.
/// 2. If the process is stopped, block the current task on the `stop_event`.
/// 3. The task will be woken when a `SIGCONT` or `SIGKILL` signal arrives.
///
/// # Implementation Details
///
/// How this function implements the process stop:
/// - Using a `block_on` function to block the current task on a future that
///   will not be ready unless the process is continued.
/// - The future is created by a `poll_fn` function, which will poll the stopped
///   state of the process.
/// - The stopped state is checked in the `poll_fn` function, and the future
///   will be ready when the stopped state is changed to other states.
///
/// This blocking future is registered in the `stop_event` of the process
/// declared in the `ProcessData` struct. It will be woken
/// if and only if a `SIGKILL` or a `SIGCONT` signal is sent to the process.
/// When a `SIGKILL` or a `SIGCONT` signal is sent to the process (see
/// `send_signal_to_process` in `core/src/task.rs`), the `stop_event` will wake
/// up this blocked future. The future then checks:
/// 1. If the process state has changed to non-stopped, return `Ready`
///    immediately.
/// 2. Otherwise, check if `SIGCONT` or `SIGKILL` is pending via `has_signal()`.
///    If found, return `Ready` to allow signal processing to continue/kill the
///    process.
///
/// # Arguments
///
/// * `curr` - The current task.
/// * `thr` - The current thread.
fn handle_stopped_state(curr: &CurrentTask, thr: &Thread) {
    // Check if process is stopped and block until continued.
    // If the process is not in the stopped state, return.
    // The stopped state should have already been set
    // if there is a signal that requests to stop the process
    // in the previous `check_signals` function call with
    // `do_stop` function in the stopped branch.
    if !thr.proc_data.proc.is_stopped() {
        return;
    }

    info!(
        "Task {} blocked (process {} stopped)",
        curr.id().as_u64(),
        thr.proc_data.proc.pid()
    );

    // Deploy an async event for actually stopping the process
    block_on(poll_fn(|cx| {
        // Fast route: only directly return `Ready` when the process's stopped state has
        // been updated to other states.
        // This check is essential for multi-threading setting, can prevent the task
        // from being blocked when the process is already continued by some other
        // threads within the same process.
        if !thr.proc_data.proc.is_stopped() {
            Poll::Ready(())
        } else {
            // This won't got executed when process is stopped until a
            // SIGCONT or SIGKILL arrives, which would change the process state
            info!(
                "Task {} blocked (process {} stopped), checking signals",
                curr.id().as_u64(),
                thr.proc_data.proc.pid()
            );
            if thr.signal.has_signal(Signo::SIGCONT)
                || thr.signal.has_signal(Signo::SIGKILL)
                || thr.proc_data.signal.has_signal(Signo::SIGCONT)
                || thr.proc_data.signal.has_signal(Signo::SIGKILL)
            {
                info!(
                    "Task {} blocked (process {} stopped), signal received",
                    curr.id().as_u64(),
                    thr.proc_data.proc.pid()
                );
                return Poll::Ready(());
            }

            info!(
                "Task {} blocked (process {} stopped), waiting for signal",
                curr.id().as_u64(),
                thr.proc_data.proc.pid()
            );

            // Register the waker for a `stop_event`
            thr.proc_data.stop_event.register(cx.waker());
            Poll::Pending
        }
    }));

    info!(
        "Task {} resumed (process {} continued)",
        curr.id().as_u64(),
        thr.proc_data.proc.pid()
    );
}
/// Stop the current process per a stopping signal.
///
/// Several procedures are involved in this function:
/// 1. Remove all SIGCONT signals pending in the process's queue.
/// 2. Remove all SIGCONT signals pending in each thread's queue.
/// 3. Record the stop signal in `ProcessData`.
/// 4. Change the state of current process to `STOPPED`.
/// 5. Notify parent process for this stoppage state change.
///
/// # Arguments
///
/// * `stop_signal` - The signal that causes the process to stop.
pub(crate) fn do_stop(stop_signal: Signo) {
    let curr = current();
    let curr_thread = curr.as_thread();
    let curr_process = &curr_thread.proc_data.proc;

    // If current process is not running, do nothing.
    if !curr_process.is_running() {
        warn!("Process {} is not running", curr_process.pid());
        return;
    }

    info!(
        "Process {} stopping due to signal {}",
        curr_process.pid(),
        stop_signal as u8
    );

    // remove all SIGCONT signals pending in the process's queue
    curr_thread.proc_data.signal.remove_signal(Signo::SIGCONT);

    // remove all SIGCONT signals pending in each thread's queue
    curr_process.threads().iter().for_each(|tid| {
        if let Ok(thread) = get_task(*tid) {
            thread.as_thread().signal.remove_signal(Signo::SIGCONT);
        }
    });

    // record the stop signal in the `ProcessSignalManager`
    curr_thread.proc_data.signal.set_stop_signal(stop_signal);

    // change the state of current process to `STOPPED`
    curr_process.transition_to_stopped();

    // notify parent process for this stoppage state change
    if let Some(parent) = curr_process.parent()
        && let Ok(parent_data) = get_process_data(parent.pid())
    {
        parent_data.child_exit_event.wake();

        // POSIX: Send SIGCHLD to parent when child stops (unless SA_NOCLDSTOP set)
        // TODO: Check SA_NOCLDSTOP flag when implemented
        let siginfo = SignalInfo::new_kernel(Signo::SIGCHLD);
        let _ = send_signal_to_process(parent.pid(), Some(siginfo));
    }
}

/// Continue the current process per a `SIGCONT` signal.
///
/// The procedure is as follows:
/// 1. Remove all stopping signals pending in the process's queue.
/// 2. Remove all stopping signals pending in each thread's queue.
/// 3. Change the state of current process to `RUNNING`.
/// 4. Resume all threads in the process.
/// 5. Notify parent process for this stoppage state change.
pub(crate) fn do_continue() {
    let curr = current();
    let curr_thread = curr.as_thread();
    let curr_proc = &curr_thread.proc_data.proc;

    // If current process is not stopped, do nothing.
    if !curr_proc.is_stopped() {
        warn!("Process {} is not stopped", curr_proc.pid());
        return;
    }

    info!("Process {} continuing due to signal", curr_proc.pid());

    // remove all stopping signals pending in the process's queue
    curr_thread.proc_data.signal.flush_stop_signals();

    // remove all stopping signals pending in each thread's queue
    for thread_pid in curr_proc.threads().iter() {
        if let Ok(thread) = get_task(*thread_pid) {
            thread.as_thread().signal.flush_stop_signals();
        }
    }

    // record the continue event in the `ProcessSignalManager`
    curr_thread.proc_data.signal.set_cont_signal();

    // change the state of current process to `RUNNING`
    curr_proc.transition_to_running();

    // wake up all threads
    for thread_pid in curr_proc.threads().iter() {
        if let Ok(thread) = get_task(*thread_pid) {
            thread.interrupt();
        }
    }

    // Notify parent process for this continuation state change
    if let Some(parent) = curr_proc.parent()
        && let Ok(parent_data) = get_process_data(parent.pid())
    {
        parent_data.child_exit_event.wake();
    }
}
