use alloc::sync::Arc;

use axerrno::{AxError, AxResult};
use axhal::time::monotonic_time_nanos;
use axsync::Mutex;
use axtask::current;
use starry_core::{
    msg::{MSG_MANAGER, MSGMAX, MSGMNB, MsgInner},
    task::AsThread,
};
use starry_process::Pid;

use super::{
    IPC_CREAT, IPC_EXCL, IPC_INFO, IPC_PRIVATE, IPC_RMID, IPC_SET, IPC_STAT, MSG_INFO, MSG_STAT,
    has_ipc_permission, next_ipc_id,
};
use crate::{
    mm::{UserPtr, nullable},
    syscall::{sys_getgid, sys_getuid},
};

bitflags::bitflags! {
    /// Flags for msgrcv
    #[derive(Debug)]
    pub struct MsgRcvFlags: i32 {
        /// Non-blocking receive (return immediately if no message)
        const IPC_NOWAIT = 0o4000;
        /// Truncate message if too long (instead of failing)
        const MSG_NOERROR = 0o10000;
        /// For internal use - mark as COPIED
        const MSG_COPY = 0o20000;
        /// Receive any message except of specified type (Linux extension)
        const MSG_EXCEPT = 0o2000;
    }
}

bitflags::bitflags! {
    /// Flags for msgsnd
    #[derive(Debug)]
    pub struct MsgSndFlags: i32 {
        /// Non-blocking send (return immediately if queue full)
        const IPC_NOWAIT = 0o4000;
    }
}

pub fn sys_msgget(key: i32, msgflg: i32) -> AxResult<isize> {
    let current = current();
    let thread = current.as_thread();
    let proc_data = &thread.proc_data;
    let current_uid = sys_getuid()? as u32;
    let current_gid = sys_getgid()? as u32;
    let current_pid = proc_data.proc.pid();

    let mut msg_manager = MSG_MANAGER.lock();

    // Check system limit
    if msg_manager.queue_count() >= starry_core::msg::MSGMNI {
        return Err(AxError::StorageFull); // ENOSPC
    }

    // Handle IPC_PRIVATE (always create new queue)
    if key == IPC_PRIVATE {
        let msqid = next_ipc_id();
        let msg_inner = Arc::new(Mutex::new(MsgInner::new(
            key,
            msqid,
            (msgflg & 0o777) as _,
            current_pid,
            current_uid,
            current_gid,
        )));

        msg_manager.insert_msqid_inner(msqid, msg_inner);
        return Ok(msqid as isize);
    }

    // Look for existing message queue
    if let Some(msqid) = msg_manager.get_msqid_by_key(key) {
        let msg_inner = msg_manager
            .get_inner_by_msqid(msqid)
            .ok_or(AxError::InvalidInput)?; // ENOENT

        let msg_inner = msg_inner.lock();

        // Check permissions
        if !has_ipc_permission(
            &msg_inner.msqid_ds.msg_perm,
            current_uid,
            current_gid,
            false,
        ) {
            return Err(AxError::PermissionDenied); // EACCES
        }

        // Check if marked for removal
        if msg_inner.mark_removed {
            return Err(AxError::InvalidInput); // EIDRM
        }

        // Check IPC_EXCL flag
        if (msgflg & IPC_EXCL) != 0 && (msgflg & IPC_CREAT) != 0 {
            return Err(AxError::AlreadyExists); // EEXIST
        }

        return Ok(msqid as isize);
    }

    // Create new message queue
    if (msgflg & IPC_CREAT) == 0 {
        return Err(AxError::InvalidInput); // ENOENT
    }

    let msqid = next_ipc_id();
    let msg_inner = Arc::new(Mutex::new(MsgInner::new(
        key,
        msqid,
        (msgflg & 0o777) as _,
        current_pid,
        current_uid,
        current_gid,
    )));

    msg_manager.insert_key_msqid(key, msqid);
    msg_manager.insert_msqid_inner(msqid, msg_inner);

    Ok(msqid as isize)
}

pub fn sys_msgsnd(msqid: i32, msgp: usize, msgsz: usize, msgflg: i32) -> AxResult<isize> {
    // MSGMAX = 8192
    if msgsz > MSGMAX {
        return Err(AxError::InvalidInput); // EINVAL
    }
    let current = current();
    let thread = current.as_thread();
    let proc_data = &thread.proc_data;
    let current_uid = sys_getuid()? as u32;
    let current_gid = sys_getgid()? as u32;
    let current_pid = proc_data.proc.pid();
    let flags = MsgSndFlags::from_bits_truncate(msgflg);

    let msg_inner = {
        let msg_manager = MSG_MANAGER.lock();
        msg_manager
            .get_inner_by_msqid(msqid)
            .ok_or(AxError::InvalidInput)? // EINVAL - queue does not exist
    };

    let mut msg_inner = msg_inner.lock();

    if !has_ipc_permission(
        &msg_inner.msqid_ds.msg_perm,
        current_uid as _,
        current_gid as _,
        true,
    ) {
        return Err(AxError::PermissionDenied); // EACCES
    }

    // read message from user space
    let mtype_ptr = UserPtr::<i64>::from(msgp);
    let mtype_ref = mtype_ptr.get_as_mut()?;
    let mtype = *mtype_ref;

    if mtype <= 0 {
        return Err(AxError::InvalidInput);
    }

    // calculate data pointer
    let data_ptr = msgp + core::mem::size_of::<i64>();

    // read data part
    let data_slice = UserPtr::<u8>::from(data_ptr).get_as_mut_slice(msgsz)?;

    // check if the message queue is marked for removal
    // Note: According to Linux manpage, both byte count and message count
    // are limited by msg_qbytes field (this appears to be the actual behavior)
    let would_exceed_bytes =
        msg_inner.total_bytes + data_slice.len() > msg_inner.msqid_ds.msg_qbytes as usize;
    let would_exceed_messages =
        (msg_inner.msqid_ds.msg_qnum + 1) as usize > msg_inner.msqid_ds.msg_qbytes as usize;

    if would_exceed_bytes || would_exceed_messages {
        // If the non-blocking flag is specified, return an error immediately
        if flags.contains(MsgSndFlags::IPC_NOWAIT) {
            return Err(AxError::WouldBlock); // EAGAIN
        }

        // TODO:
        // Otherwise, block and wait (blocking logic needs to be implemented
        // here) In the actual implementation, this should:
        // - Add the current task to the wait queue
        // - Yield the CPU and wait to be woken up when there is space in the
        //   queue
        // - After being woken up, recheck the condition
        // Note: It may be interrupted by a signal returning EINTR, or the queue
        // may be deleted returning EIDRM
    }

    msg_inner.enqueue_message(mtype, data_slice)?;

    msg_inner.msqid_ds.msg_lspid = current_pid as _;

    msg_inner.msqid_ds.msg_stime = monotonic_time_nanos() as _;

    // attention:msg_qnum and msg_cbytes updated in enqueue_message

    // TODO:
    // If there are processes waiting to receive messages, wake them up
    // In the actual implementation, this should:
    // - Check if there are tasks in the message queue's wait queue
    // - If so, wake up these tasks
    Ok(0)
}

pub fn sys_msgrcv(
    msqid: i32,
    msgp: usize,
    msgsz: usize,
    msgtyp: i64,
    msgflg: i32,
) -> AxResult<isize> {
    // Parse flags and get current process information

    let flags = MsgRcvFlags::from_bits_truncate(msgflg);
    let current = current();
    let thread = current.as_thread();
    let proc_data = &thread.proc_data;
    let current_uid = sys_getuid()? as u32;
    let current_gid = sys_getgid()? as u32;
    let current_pid = proc_data.proc.pid();

    // Check validity of flag combinations
    if flags.contains(MsgRcvFlags::MSG_COPY) {
        if !flags.contains(MsgRcvFlags::IPC_NOWAIT) {
            return Err(AxError::InvalidInput); // EINVAL - MSG_COPY must be used with IPC_NOWAIT
        }
        if flags.contains(MsgRcvFlags::MSG_EXCEPT) {
            return Err(AxError::InvalidInput); // EINVAL - MSG_COPY and MSG_EXCEPT are mutually exclusive
        }
    }

    // Get the message queue
    let msg_inner = {
        let msg_manager = MSG_MANAGER.lock();
        msg_manager
            .get_inner_by_msqid(msqid)
            .ok_or(AxError::InvalidInput)? // EINVAL
    };

    let mut msg_inner = msg_inner.lock();

    // Permission check
    if !has_ipc_permission(
        &msg_inner.msqid_ds.msg_perm,
        current_uid as _,
        current_gid as _,
        false,
    ) {
        return Err(AxError::PermissionDenied); // EACCES
    }

    if msg_inner.mark_removed {
        return Err(AxError::InvalidInput); // EIDRM
    }

    // Message matching logic (distinguish between MSG_COPY and normal mode)
    let (mtype, data, should_remove) = if flags.contains(MsgRcvFlags::MSG_COPY) {
        // MSG_COPY mode: msgtyp is the message index
        let index = msgtyp as usize;

        // Check if the index is valid
        if index >= msg_inner.get_total_message_count() {
            return Err(AxError::NoMemory); // ENOMSG - index out of range
        }

        // Get a copy of the message (do not remove)
        let message = msg_inner
            .get_message_by_index(index)
            .ok_or(AxError::NoMemory)?; // ENOMSG

        (message.mtype, message.data.clone(), false) // should_remove = false
    } else {
        // Normal mode: msgtyp is the message type
        let matched_message = match msgtyp {
            0 => msg_inner.find_first_message(), // First message
            typ if typ > 0 => {
                if flags.contains(MsgRcvFlags::MSG_EXCEPT) {
                    msg_inner.find_message_not_equal(typ) // Type not equal to msgtyp
                } else {
                    msg_inner.find_message_by_type(typ) // Type equal to msgtyp
                }
            }
            typ if typ < 0 => {
                let abs_typ = typ.abs();
                msg_inner.find_message_less_equal(abs_typ) // Type ≤ |msgtyp|
            }
            _ => None,
        };

        // Handle no message situation
        let (mtype, data_slice) = match matched_message {
            Some((mtype, data_slice)) => (mtype, data_slice),
            None => {
                if flags.contains(MsgRcvFlags::IPC_NOWAIT) {
                    return Err(AxError::NoMemory); // ENOMSG
                }

                // TODO:
                // The complete implementation should:
                // - Add the current task to the receive wait queue
                // - Block and wait, possibly interrupted by signals (EINTR) or queue removal
                //   (EIDRM)
                // Simplified: blocking is not supported, directly return an error
                return Err(AxError::NoMemory);
            }
        };

        // Need to convert &[u8] to Vec<u8>
        let data = data_slice.to_vec();
        (mtype, data, true) // should_remove = true
    };

    // Message size check
    if data.len() > msgsz {
        if flags.contains(MsgRcvFlags::MSG_NOERROR) {
            // MSG_NOERROR: Truncate the message and continue
        } else {
            // Without MSG_NOERROR: return an error
            // Note: If in normal mode, the message has not been removed, so no need to
            // restore
            return Err(AxError::InvalidInput); // E2BIG
        }
    }

    // Remove the message from the queue (normal mode only)
    if should_remove {
        msg_inner.remove_matched_message_by_type(mtype, &data)?;
    }

    // Write mtype
    let mtype_ptr = UserPtr::<i64>::from(msgp);
    let mtype_ref = mtype_ptr.get_as_mut()?;
    *mtype_ref = mtype;

    // Write data part
    let data_ptr = msgp + core::mem::size_of::<i64>();
    let copy_len = data.len().min(msgsz);
    let user_data = UserPtr::<u8>::from(data_ptr).get_as_mut_slice(copy_len)?;
    user_data.copy_from_slice(&data[..copy_len]);

    // Update queue statistics (normal mode only)
    if should_remove {
        msg_inner.msqid_ds.msg_lrpid = current_pid as _;
        msg_inner.msqid_ds.msg_rtime = monotonic_time_nanos() as _;

        // TODO:
        // Wake up waiting senders (Simplified: not implemented)
        // while let Some(task) = msg_inner.send_wait_queue.pop_front() {
        //     wakeup(task);
        // }
    } else {
        // MSG_COPY mode: only update last receiver info, do not update queue statistics
        msg_inner.msqid_ds.msg_lrpid = current_pid as _;
        msg_inner.msqid_ds.msg_rtime = monotonic_time_nanos() as _;
    }

    Ok(copy_len as isize)
}

pub fn sys_msgctl(msqid: i32, cmd: i32, buf: usize) -> AxResult<isize> {
    //  Get current process information
    let current_uid = sys_getuid()? as u32;
    let current_gid = sys_getgid()? as u32;
    let is_privileged = current_uid == 0; // root user check

    // Validate command code
    if cmd != IPC_STAT
        && cmd != IPC_SET
        && cmd != IPC_RMID
        && cmd != IPC_INFO
        && cmd != MSG_INFO
        && cmd != MSG_STAT
    {
        // Simplified: do not support some Linux extensions
        return Err(AxError::InvalidInput); // EINVAL
    }

    // IPC_INFO (put before looking up the queue!)
    if cmd == IPC_INFO {
        // IPC_INFO uses msqid=0, no actual queue needed
        // Return system-level information
        #[repr(C)]
        struct MsgInfo {
            msgpool: i32,
            msgmap: i32,
            msgmax: i32,
            msgmnb: i32,
            msgmni: i32,
            msgssz: i32,
            msgtql: i32,
            msgseg: u16,
        }

        let info = MsgInfo {
            msgpool: 0,
            msgmap: 0,
            msgmax: starry_core::msg::MSGMAX as i32,
            msgmnb: starry_core::msg::MSGMNB as i32,
            msgmni: starry_core::msg::MSGMNI as i32,
            msgssz: 0,
            msgtql: 0,
            msgseg: 0,
        };

        // Copy to user space
        let user_ptr = core::ptr::NonNull::new(buf as *mut MsgInfo).ok_or(AxError::InvalidInput)?;
        unsafe {
            user_ptr.as_ptr().copy_from(core::ptr::addr_of!(info), 1);
        }
        return Ok(0);
    }

    // MSG_INFO (put before looking up the queue!)
    if cmd == MSG_INFO {
        let msg_manager = MSG_MANAGER.lock();
        // Manually create IpcPerm
        let msg_perm = starry_core::shm::IpcPerm {
            key: 0,
            uid: current_uid,
            gid: current_gid,
            cuid: current_uid,
            cgid: current_gid,
            mode: 0o600,
            pad: 0,
            seq: 0,
            unused0: 0,
            unused1: 0,
        };

        // Create a temporary msqid_ds to return information
        let info_ds = starry_core::msg::MsqidDs {
            msg_perm,
            msg_stime: 0,
            msg_rtime: 0,
            msg_ctime: 0,
            msg_cbytes: msg_manager.total_bytes() as u64,
            // Use msg_qnum to return the number of allocated queues
            msg_qnum: msg_manager.queue_count() as u64,
            // Use msg_qbytes to return system limits or usage
            msg_qbytes: starry_core::msg::MSGMNB as u64,
            msg_lspid: Pid::from(0u32) as _,
            msg_lrpid: Pid::from(0u32) as _,
        };

        // Copy to user space
        let user_ptr = UserPtr::<starry_core::msg::MsqidDs>::from(buf);
        if let Some(user_buf) = nullable!(user_ptr.get_as_mut())? {
            *user_buf = info_ds;
        }

        // Return the current number of allocated queues
        return Ok(msg_manager.queue_count() as isize);
    }
    // MSG_STAT handling
    if cmd == MSG_STAT {
        let msg_manager = MSG_MANAGER.lock();

        let mut current_index = 0;
        for (&actual_msqid, inner) in &msg_manager.msqid_inner {
            let guard = inner.lock();
            if !guard.mark_removed {
                if current_index == msqid as usize {
                    let user_ptr = UserPtr::<starry_core::msg::MsqidDs>::from(buf);
                    if let Some(user_buf) = nullable!(user_ptr.get_as_mut())? {
                        *user_buf = guard.msqid_ds;
                    }

                    return Ok(actual_msqid as isize);
                }
                current_index += 1;
            }
        }
        return Err(AxError::InvalidInput);
    }

    // Find message queue by msqid
    let msg_inner = {
        let msg_manager = MSG_MANAGER.lock();
        msg_manager
            .get_inner_by_msqid(msqid)
            .ok_or(AxError::InvalidInput)? // EINVAL - Queue does not exist
    };

    // Lock the internal structure of the queue
    let mut msg_inner = msg_inner.lock();
    // Check if the queue is marked as removed
    if msg_inner.mark_removed {
        return Err(AxError::InvalidInput); // EIDRM - Queue has been removed
    }
    if cmd == IPC_STAT {
        // Check read permissions
        if !has_ipc_permission(
            &msg_inner.msqid_ds.msg_perm,
            current_uid,
            current_gid,
            false,
        ) {
            return Err(AxError::PermissionDenied); // EACCES
        }

        // Copy queue status to user space
        let user_ptr = UserPtr::<starry_core::msg::MsqidDs>::from(buf);
        if let Some(user_buf) = nullable!(user_ptr.get_as_mut())? {
            *user_buf = msg_inner.msqid_ds;
        }

        return Ok(0);
    }
    if cmd == IPC_SET {
        // Check permissions (owner, creator, or privileged user)
        let is_owner = current_uid == msg_inner.msqid_ds.msg_perm.uid;
        let is_creator = current_uid == msg_inner.msqid_ds.msg_perm.cuid;

        if !is_privileged && !is_owner && !is_creator {
            return Err(AxError::PermissionDenied); // EPERM
        }

        // Read new settings from user space
        let user_ptr = UserPtr::<starry_core::msg::MsqidDs>::from(buf);
        if let Some(user_buf) = nullable!(user_ptr.get_as_mut())? {
            // Update permission information (fields allowed by man-page)
            msg_inner.msqid_ds.msg_perm.uid = user_buf.msg_perm.uid;
            msg_inner.msqid_ds.msg_perm.gid = user_buf.msg_perm.gid;
            msg_inner.msqid_ds.msg_perm.mode = user_buf.msg_perm.mode & 0o777; // Only take permission bits

            // Update queue size limit (requires privilege check)
            if user_buf.msg_qbytes != msg_inner.msqid_ds.msg_qbytes {
                if user_buf.msg_qbytes > MSGMNB as _ && !is_privileged {
                    return Err(AxError::PermissionDenied); // EPERM - requires privilege to exceed MSGMNB
                }
                msg_inner.msqid_ds.msg_qbytes = user_buf.msg_qbytes;
            }

            // Update modification time
            msg_inner.msqid_ds.msg_ctime = monotonic_time_nanos() as _;
        }

        return Ok(0);
    }
    if cmd == IPC_RMID {
        // Check permissions (owner, creator, or privileged user)
        let is_owner = current_uid == msg_inner.msqid_ds.msg_perm.uid;
        let is_creator = current_uid == msg_inner.msqid_ds.msg_perm.cuid;

        if !is_privileged && !is_owner && !is_creator {
            return Err(AxError::PermissionDenied); // EPERM
        }

        // Mark the queue as removed
        msg_inner.mark_removed = true;

        // If the queue is empty, delete it immediately
        if msg_inner.msqid_ds.msg_qnum == 0 {
            drop(msg_inner); // Release the lock to avoid deadlock

            let mut msg_manager = MSG_MANAGER.lock();
            msg_manager.remove_msqid(msqid);

            // TODO:
            // Wake up all waiting processes (simplified: not implemented yet)
            // According to man-page: wake up all waiting readers and writers (returning
            // EIDRM error)

            return Ok(0);
        }

        // If the queue is not empty, only mark it as removed and wait for all messages
        // to be taken before automatic deletion Update modification time
        msg_inner.msqid_ds.msg_ctime = monotonic_time_nanos() as _;

        return Ok(0);
    }
    // Currently unsupported operations
    // IPC_INFO, MSG_INFO, MSG_STAT and other Linux-specific extensions
    // These Linux-specific extensions are not implemented for now because the basic
    // operations are sufficient and these are not POSIX standard They can be
    // implemented later to support tools like ipcs
    Err(AxError::InvalidInput) // EINVAL
}
