//! Message queue management.

use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};

use axerrno::{AxError, AxResult};
use axhal::time::monotonic_time_nanos;
use axsync::Mutex;
use linux_raw_sys::general::*;
use starry_process::Pid;

use super::shm::IpcPerm;

/// Data structure describing a message queue.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MsqidDs {
    /// operation permission struct
    pub msg_perm: IpcPerm,
    /// time of last msgsnd()
    pub msg_stime: __kernel_time_t,
    /// time of last msgrcv()
    pub msg_rtime: __kernel_time_t,
    /// time of last change by msgctl()
    pub msg_ctime: __kernel_time_t,
    /// current number of bytes on queue
    pub msg_cbytes: __kernel_size_t,
    /// number of messages in queue
    pub msg_qnum: __kernel_size_t,
    /// max number of bytes on queue
    pub msg_qbytes: __kernel_size_t,
    /// pid of last msgsnd()
    pub msg_lspid: __kernel_pid_t,
    /// pid of last msgrcv()
    pub msg_lrpid: __kernel_pid_t,
}

impl MsqidDs {
    fn new(key: i32, mode: __kernel_mode_t, pid: __kernel_pid_t, uid: u32, gid: u32) -> Self {
        Self {
            msg_perm: IpcPerm {
                key,
                uid,
                gid,
                cuid: uid,
                cgid: gid,
                mode,
                seq: 0,
                pad: 0,
                unused0: 0,
                unused1: 0,
            },
            msg_stime: 0,
            msg_rtime: 0,
            msg_ctime: monotonic_time_nanos() as __kernel_time_t,
            msg_cbytes: 0,
            msg_qnum: 0,
            msg_qbytes: MSGMNB as __kernel_size_t,
            msg_lspid: pid,
            msg_lrpid: pid,
        }
    }
}

/// Single message in the queue
pub struct Message {
    /// message type
    pub mtype: i64,
    /// message data
    pub data: Vec<u8>,
    /// timestamp when message was sent
    pub timestamp: u64,
}

/// This struct is used to maintain the message queue in kernel.
pub struct MsgInner {
    /// Message queue identifier
    pub msqid: i32,
    /// User-provided key
    pub key: i32,
    /// Message queue data structure
    pub msqid_ds: MsqidDs,
    /// Queue of messages
    pub messages: BTreeMap<i64, Vec<Message>>, // mtype -> messages of that type
    /// Total bytes in queue
    pub total_bytes: usize,
    /// Marked for removal
    pub mark_removed: bool,
}

impl MsgInner {
    /// Creates a new [`MsgInner`].
    pub fn new(key: i32, msqid: i32, mode: __kernel_mode_t, pid: Pid, uid: u32, gid: u32) -> Self {
        MsgInner {
            msqid,
            key,
            msqid_ds: MsqidDs::new(key, mode, pid as __kernel_pid_t, uid, gid),
            messages: BTreeMap::new(),
            total_bytes: 0,
            mark_removed: false,
        }
    }

    /// Add a message to the queue
    pub fn enqueue_message(&mut self, mtype: i64, data: &[u8]) -> AxResult<()> {
        // Check queue size limits
        if self.total_bytes + data.len() > self.msqid_ds.msg_qbytes as usize {
            return Err(AxError::NoMemory); // ENOSPC
        }

        let message = Message {
            mtype,
            data: data.to_vec(),
            timestamp: monotonic_time_nanos(),
        };

        self.messages.entry(mtype).or_default().push(message);
        self.total_bytes += data.len();
        self.msqid_ds.msg_cbytes += data.len() as __kernel_size_t;
        self.msqid_ds.msg_qnum += 1;

        Ok(())
    }

    /// Remove and return a message from the queue
    pub fn dequeue_message(&mut self, msgtyp: i64) -> AxResult<(i64, Vec<u8>)> {
        let matched_message = match msgtyp {
            0 => self.find_first_message(),
            typ if typ > 0 => self.find_message_by_type(typ),
            typ if typ < 0 => {
                let abs_typ = typ.abs();
                self.find_message_less_equal(abs_typ)
            }
            _ => None,
        };

        // Process the found message
        let (mtype, _data_slice) = matched_message.ok_or(AxError::NoMemory)?;

        // Remove the message from the queue
        if let Some(removed_msg) = self.remove_first_message_of_type(mtype) {
            Ok((removed_msg.mtype, removed_msg.data))
        } else {
            Err(AxError::NoMemory)
        }
    }

    /// Find the first message (without removing)
    pub fn find_first_message(&self) -> Option<(i64, &[u8])> {
        for (&mtype, messages) in &self.messages {
            if let Some(message) = messages.first() {
                return Some((mtype, &message.data[..]));
            }
        }
        None
    }

    /// Find message by type (without removing)
    pub fn find_message_by_type(&self, msgtyp: i64) -> Option<(i64, &[u8])> {
        self.messages
            .get(&msgtyp)
            .and_then(|msgs| msgs.first())
            .map(|msg| (msgtyp, &msg.data[..]))
    }

    /// Find the first message with a type not equal to the specified value
    /// (without removing)
    pub fn find_message_not_equal(&self, msgtyp: i64) -> Option<(i64, &[u8])> {
        for (&mtype, messages) in &self.messages {
            if mtype != msgtyp
                && let Some(message) = messages.first()
            {
                return Some((mtype, &message.data[..]));
            }
        }
        None
    }

    /// Find the first message with a type less than or equal to |msgtyp|
    /// (without removing)
    pub fn find_message_less_equal(&self, abs_typ: i64) -> Option<(i64, &[u8])> {
        let mut candidate_type = None;

        // Find the smallest type among all types ≤ abs_typ
        for (&mtype, messages) in &self.messages {
            if mtype <= abs_typ
                && !messages.is_empty()
                && candidate_type.is_none_or(|candidate| mtype < candidate)
            {
                candidate_type = Some(mtype);
            }
        }

        // Return the found message (without removing)
        if let Some(mtype) = candidate_type {
            self.messages
                .get(&mtype)
                .and_then(|msgs| msgs.first())
                .map(|msg| (mtype, &msg.data[..]))
        } else {
            None
        }
    }

    /// Get total number of messages in the queue (for MSG_COPY)
    pub fn get_total_message_count(&self) -> usize {
        self.messages.values().map(|msgs| msgs.len()).sum()
    }

    /// Get message by index (for MSG_COPY)
    pub fn get_message_by_index(&self, index: usize) -> Option<&Message> {
        let mut current_index = 0;

        // Iterate over all messages in order of message type
        for messages in self.messages.values() {
            if index < current_index + messages.len() {
                return messages.get(index - current_index);
            }
            current_index += messages.len();
        }
        None
    }

    /// Remove the first message of the specified type
    pub fn remove_first_message_of_type(&mut self, mtype: i64) -> Option<Message> {
        if let Some(messages) = self.messages.get_mut(&mtype) {
            let removed_msg = messages.remove(0);

            // Update statistics
            self.total_bytes -= removed_msg.data.len();
            self.msqid_ds.msg_cbytes -= removed_msg.data.len() as __kernel_size_t;
            self.msqid_ds.msg_qnum -= 1;

            // If the message list of this type is empty, remove the entire type entry
            if messages.is_empty() {
                self.messages.remove(&mtype);
            }
            return Some(removed_msg);
        }
        None
    }

    /// Remove the first message of the specified type with matching content
    pub fn remove_matched_message_by_type(&mut self, mtype: i64, data: &[u8]) -> AxResult<()> {
        if let Some(messages) = self.messages.get_mut(&mtype) {
            // Find the first message with matching content and type
            if let Some(pos) = messages.iter().position(|msg| msg.data == data) {
                let removed_msg = messages.remove(pos);

                // Update core queue statistics in the removal method
                self.total_bytes -= removed_msg.data.len();
                self.msqid_ds.msg_cbytes -= removed_msg.data.len() as __kernel_size_t;
                self.msqid_ds.msg_qnum -= 1;

                // If the message list of this type is empty, remove the entire type entry
                if messages.is_empty() {
                    self.messages.remove(&mtype);
                }

                return Ok(());
            }
        }
        Err(AxError::NoMemory)
    }
}

/// Message queue manager
pub struct MsgManager {
    /// key -> msqid mapping
    key_msqid: BTreeMap<i32, i32>,
    /// msqid -> message queue inner structure
    pub msqid_inner: BTreeMap<i32, Arc<Mutex<MsgInner>>>,
    /// Current number of message queues
    queue_count: usize,
}

impl MsgManager {
    const fn new() -> Self {
        MsgManager {
            key_msqid: BTreeMap::new(),
            msqid_inner: BTreeMap::new(),
            queue_count: 0,
        }
    }

    /// Returns the message queue ID associated with the given key.
    pub fn get_msqid_by_key(&self, key: i32) -> Option<i32> {
        self.key_msqid.get(&key).cloned()
    }

    /// Returns the message queue inner structure associated with the given ID.
    pub fn get_inner_by_msqid(&self, msqid: i32) -> Option<Arc<Mutex<MsgInner>>> {
        self.msqid_inner.get(&msqid).cloned()
    }

    /// Inserts a mapping from a key to a message queue ID.
    pub fn insert_key_msqid(&mut self, key: i32, msqid: i32) {
        self.key_msqid.insert(key, msqid);
    }

    /// Inserts a mapping from a message queue ID to its inner structure.
    pub fn insert_msqid_inner(&mut self, msqid: i32, msg_inner: Arc<Mutex<MsgInner>>) {
        self.msqid_inner.insert(msqid, msg_inner);
        self.queue_count += 1;
    }

    /// Returns the current number of message queues.
    pub fn queue_count(&self) -> usize {
        self.queue_count
    }

    /// Remove a message queue
    pub fn remove_msqid(&mut self, msqid: i32) {
        self.key_msqid.retain(|_, &mut v| v != msqid);
        self.msqid_inner.remove(&msqid);
        self.queue_count = self.queue_count.saturating_sub(1);
    }

    /// get total bytes in all queues
    pub fn total_bytes(&self) -> usize {
        let mut total = 0;
        for (_, inner) in &self.msqid_inner {
            let guard = inner.lock();
            if !guard.mark_removed {
                total += guard.total_bytes;
            }
        }
        total
    }

    /// get total messages in all queues
    pub fn total_messages(&self) -> usize {
        let mut total = 0;
        for (_, inner) in &self.msqid_inner {
            let guard = inner.lock();
            if !guard.mark_removed {
                total += guard.msqid_ds.msg_qnum as usize;
            }
        }
        total
    }
}

/// System limits
/// Maximum number of message queues
pub const MSGMNI: usize = 32000;
/// Maximum bytes in a message queue
pub const MSGMNB: usize = 16384;
/// Maximum size of a single message
pub const MSGMAX: usize = 8192;

/// Global message queue manager
pub static MSG_MANAGER: Mutex<MsgManager> = Mutex::new(MsgManager::new());
