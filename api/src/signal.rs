use core::sync::atomic::{AtomicBool, Ordering};

use axerrno::AxResult;
use axhal::uspace::UserContext;
use axtask::current;
use starry_core::task::{AsThread, Thread};
use starry_signal::{SignalOSAction, SignalSet, Signo};

use crate::task::{do_continue, do_exit, do_stop};

/// Check, and handle non-customized signals for a thread.
///
/// This function checks the signals for a thread and handles them if any are
/// pending. The procedure of signal handling is as follows:
/// 1. Do a pre-check for `SIGCONT`, if there is a `SIGCONT` and current process
///    is stopped, continue the process no matter the signal is blocked/ignored
///    or not.
/// 2. Check the signal queue for the thread and handle the signal if any is
///    pending. If the signal is unblocked, it would be removed from the thread-
///    level signal queue and the signal would be handled following.
/// 3. An early-return for a special case of `SIGCONT` where its corresponding
///    disposition is set to be `SIG_IGN` (ignored). Since the process has
///    already been continued in step 1, we return `true` without further
///    processing.
/// 4. Handle an unexpected case where a signal has no OS action (should not
///    happen for normal signals). Log a warning and return `false`.
/// 5. Handle the signal's OS-level action: Terminate, CoreDump, Stop, Continue,
///    or Handler (which has already set up the user context for handler
///    execution).
///
/// # Arguments
///
/// * `thr` - The thread to check signals for.
/// * `uctx` - The user context to use for signal delivery.
/// * `restore_blocked` - The signal set to restore after signal delivery.
///
/// # Returns
///
/// * `true` - If the signal is handled.
/// * `false` - If the signal is not handled.
pub fn check_signals(
    thr: &Thread,
    uctx: &mut UserContext,
    restore_blocked: Option<SignalSet>,
) -> bool {
    // Per POSIX.1-2024, when SIGCONT is sent to a stopped process:
    // 1. The process MUST continue (transition from STOPPED to RUNNING), even if:
    //    - SIGCONT is blocked by all threads
    //    - SIGCONT disposition is SIG_IGN (ignored)
    //    - SIGCONT has a custom handler registered
    // 2. The signal remains pending if blocked (delivered when unblocked later)
    // 3. The handler executes after continuation (if not ignored)
    //
    // This pre-check ensures the side effect (continuing) happens before signal
    // delivery as a must, which may be deferred if the signal is blocked.
    //
    // We only check with `has_signal()`, not `dequeue_signal()`, because blocked
    // signals won't be dequeued but must still trigger the continue operation.
    if thr.proc_data.proc.is_stopped() {
        if thr.signal.has_signal(Signo::SIGCONT) || thr.proc_data.signal.has_signal(Signo::SIGCONT)
        {
            info!(
                "Process {} continuing due to pending SIGCONT (may be blocked)",
                thr.proc_data.proc.pid()
            );
            do_continue();
        }
    }

    // A side effect of `check_signals` here woould be:
    // if the signal is unblocked, it would be removed from the thread-level signal
    // queue, and the signal would be handled following.
    let Some((sig, os_action)) = thr.signal.check_signals(uctx, restore_blocked) else {
        return false;
    };

    let signo = sig.signo();

    // Special case:
    // SIGCONT with ignored disposition.
    // `os_action` is None, but we still need to continue.
    // Since the `do_continue` is called initially, we may safely return.
    if signo == Signo::SIGCONT && os_action.is_none() {
        info!(
            "Process {} continuing due to ignored SIGCONT with stopped: {}",
            thr.proc_data.proc.pid(),
            thr.proc_data.proc.is_stopped()
        );

        return true;
    }

    // Handle normal signals with OS actions
    let Some(os_action) = os_action else {
        // This shouldn't happen for other signals, but handle gracefully
        warn!(
            "Process {} received signal {} with no OS action",
            thr.proc_data.proc.pid(),
            signo as u8
        );
        return false;
    };

    match os_action {
        SignalOSAction::Terminate => {
            info!(
                "Process {} terminating due to signal",
                thr.proc_data.proc.pid()
            );
            do_exit(signo as i32, true);
        }
        SignalOSAction::CoreDump => {
            // TODO: implement core dump
            do_exit(128 + signo as i32, true);
        }
        SignalOSAction::Stop => {
            info!("Process {} stopped due to signal", thr.proc_data.proc.pid());
            do_stop(signo);
        }
        SignalOSAction::Continue => {
            info!(
                "Process {} continuing due to signal (handled by pre-check)",
                thr.proc_data.proc.pid()
            );
        }
        SignalOSAction::Handler => {
            info!("Process {} handling signal", thr.proc_data.proc.pid());
        }
    }
    true
}

static BLOCK_NEXT_SIGNAL_CHECK: AtomicBool = AtomicBool::new(false);

pub fn block_next_signal() {
    BLOCK_NEXT_SIGNAL_CHECK.store(true, Ordering::SeqCst);
}

pub fn unblock_next_signal() -> bool {
    BLOCK_NEXT_SIGNAL_CHECK.swap(false, Ordering::SeqCst)
}

pub fn with_replacen_blocked<R>(
    blocked: Option<SignalSet>,
    f: impl FnOnce() -> AxResult<R>,
) -> AxResult<R> {
    let curr = current();
    let sig = &curr.as_thread().signal;

    let old_blocked = blocked.map(|set| sig.set_blocked(set));
    f().inspect(|_| {
        if let Some(old) = old_blocked {
            sig.set_blocked(old);
        }
    })
}
