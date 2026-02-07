use alloc::{
    borrow::Cow,
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Weak},
    vec::Vec,
};
use core::{
    any::Any,
    mem::size_of,
    sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering},
};

use axerrno::{AxError, AxResult};
use axfs::ROOT_FS_CONTEXT;
use axfs_ng_vfs::Location;
use axio::{BufMut, Write};
use axpoll::{IoEvents, PollSet, Pollable};
use axsync::Mutex;
use axtask::future::{block_on, poll_io};
use bitflags::bitflags;
use lazy_static::lazy_static;

use crate::file::{FileLike, Kstat, SealedBuf, SealedBufMut};

/// ========== Inotify event flags ==========
pub const IN_IGNORED: u32 = 0x00008000; // File was ignored
/// ==========  Flags for inotify_init1()==========
pub const IN_CLOEXEC: u32 = 0o2000000; // 02000000, Set FD_CLOEXEC
pub const IN_NONBLOCK: u32 = 0o0004000; // 00004000, Set O_NONBLOCK

// flags for inotify_syscalls
bitflags! {
    #[derive(Debug, Clone, Copy, Default)]
    pub struct InotifyFlags: u32 {
        /// Create a file descriptor that is closed on `exec`.
        const CLOEXEC = IN_CLOEXEC;
        /// Create a non-blocking inotify instance.
        const NONBLOCK = IN_NONBLOCK;
    }
}

/// inotifyEvent(Memory layout fully compatible with Linux)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InotifyEvent {
    pub wd: i32,     // Watch descriptor
    pub mask: u32,   // Mask describing event
    pub cookie: u32, // Unique cookie associating related events
    pub len: u32,    /* Size of name field  (including null terminator)
                      * note: the name field is a variable-length array and is not contained in this struct */
}

/// Monitoring data of each inode（stored in Location::user_data()）
#[derive(Default)]
struct InodeWatchData {
    // key: watch descriptor, value: (instance_id, event mask)
    watches: Mutex<BTreeMap<i32, (u64, u32)>>, // Using Mutex to wrap
}
impl InodeWatchData {
    fn add_watch(&self, wd: i32, instance_id: u64, mask: u32) {
        self.watches.lock().insert(wd, (instance_id, mask));
    }

    fn remove_watch(&self, wd: i32) -> bool {
        self.watches.lock().remove(&wd).is_some()
    }

    fn is_empty(&self) -> bool {
        self.watches.lock().is_empty()
    }
}

/// inotify instance
pub struct InotifyInstance {
    // event_queue:save serialized event data
    event_queue: Mutex<VecDeque<Vec<u8>>>,

    // Added: weak reference from wd to Location (for quick lookup and path retrieval)
    wd_to_location: Mutex<BTreeMap<i32, Weak<Location>>>,

    next_wd: AtomicI32,

    // Instance ID (unique identifier)
    instance_id: u64,

    // blocking/non-blocking mode
    non_blocking: AtomicBool,

    // poll support
    poll_set: PollSet,
}

impl InotifyInstance {
    /// create new instance
    pub fn new(flags: i32) -> AxResult<Arc<Self>> {
        static NEXT_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

        let flags = flags as u32;
        // verify flags
        let valid_flags = IN_NONBLOCK | IN_CLOEXEC;
        if flags & !valid_flags != 0 {
            return Err(AxError::InvalidInput);
        }

        let non_blocking = (flags & IN_NONBLOCK) != 0;
        let instance_id = NEXT_INSTANCE_ID.fetch_add(1, Ordering::Relaxed);

        let instance = Arc::new(Self {
            event_queue: Mutex::new(VecDeque::new()),
            wd_to_location: Mutex::new(BTreeMap::new()),
            next_wd: AtomicI32::new(1),
            instance_id,
            non_blocking: AtomicBool::new(non_blocking),
            poll_set: PollSet::new(),
        });

        // Registered to global manager
        INOTIFY_MANAGER.register(instance_id, Arc::clone(&instance));

        Ok(instance)
    }

    /// Serialized events are in binary format for users to read with char[]
    fn serialize_event(event: &InotifyEvent, name: Option<&str>) -> Vec<u8> {
        // +1 for null terminator
        let name_len = name.map_or(0, |s| s.len() + 1);
        let total_len = size_of::<InotifyEvent>() + name_len;

        // Linux requires events to be 4-byte aligned
        let aligned_len = (total_len + 3) & !3;

        let mut buf = Vec::with_capacity(aligned_len);

        // Write event header (native byte order, matching architecture)
        buf.extend_from_slice(&event.wd.to_ne_bytes());
        buf.extend_from_slice(&event.mask.to_ne_bytes());
        buf.extend_from_slice(&event.cookie.to_ne_bytes());
        buf.extend_from_slice(&(name_len as u32).to_ne_bytes());

        // Write filename (if any)
        if let Some(name) = name {
            buf.extend_from_slice(name.as_bytes());
            buf.push(0); // null terminator

            // Padding for alignment (using null bytes)
            let padding = aligned_len - total_len;
            buf.resize(buf.len() + padding, 0);
        }

        buf
    }

    /// add watch for a path
    /// Returns watch descriptor (wd)
    pub fn add_watch(&self, path: &str, mask: u32) -> AxResult<i32> {
        // Convert path to Location
        let location = self.resolve_path(path)?;
        // Generate a new watch descriptor
        let wd = self.next_wd.fetch_add(1, Ordering::Relaxed);
        if wd == i32::MAX {
            return Err(AxError::StorageFull);
        }

        // Get user_data of location
        let mut user_data = location.user_data();

        // Get or create InodeWatchData
        // Use get_or_insert_with to get Arc<InodeWatchData>
        let inode_data = user_data.get_or_insert_with(InodeWatchData::default);

        // Store watch info: wd -> (instance_id, mask)
        inode_data.add_watch(wd, self.instance_id, mask);

        // Store reverse mapping: wd -> location
        self.wd_to_location
            .lock()
            .insert(wd, Arc::downgrade(&location));

        Ok(wd)
    }

    /// remove watch (generate IN_IGNORED event)
    pub fn remove_watch(&self, wd: i32) -> AxResult<()> {
        // Get location from wd_to_location
        let location_weak = {
            let mut wd_map = self.wd_to_location.lock();
            wd_map.remove(&wd).ok_or(AxError::InvalidInput)?
        };

        // Generate IN_IGNORED event
        let event = InotifyEvent {
            wd,
            mask: IN_IGNORED,
            cookie: 0,
            len: 0,
        };
        let event_data = Self::serialize_event(&event, None);
        self.push_event(event_data);

        // If location exists, remove watch from its user_data
        if let Some(location) = location_weak.upgrade() {
            let user_data = location.user_data();

            if let Some(inode_data) = user_data.get::<InodeWatchData>() {
                // Remove watch
                inode_data.remove_watch(wd);
                // If no more watches, remove InodeWatchData
                if inode_data.is_empty() {
                    // Actually TypeMap has no remove, can only leave empty
                }
            }
        }

        Ok(())
    }

    /// Push event to queue
    fn push_event(&self, event_data: Vec<u8>) {
        let mut queue = self.event_queue.lock();
        queue.push_back(event_data);
        self.poll_set.wake();
    }

    fn resolve_path(&self, path: &str) -> AxResult<Arc<Location>> {
        let fs_ctx = ROOT_FS_CONTEXT.get().ok_or(AxError::NotFound)?;
        let loc = fs_ctx.resolve(path).map_err(|_| AxError::NotFound)?;
        Ok(Arc::new(loc))
    }
}

impl FileLike for InotifyInstance {
    fn read(&self, dst: &mut SealedBufMut) -> axio::Result<usize> {
        block_on(poll_io(self, IoEvents::IN, self.nonblocking(), || {
            let mut queue = self.event_queue.lock();

            if queue.is_empty() {
                return Err(AxError::WouldBlock);
            }

            let mut bytes_written = 0;

            // Write as many events as possible without exceeding the buffer
            while let Some(event_data) = queue.front() {
                if dst.remaining_mut() < event_data.len() {
                    break;
                }

                dst.write(event_data)?;
                bytes_written += event_data.len();
                queue.pop_front();
            }

            if bytes_written > 0 {
                Ok(bytes_written)
            } else {
                // Buffer too small to write a complete event
                Err(AxError::InvalidInput)
            }
        }))
    }

    fn write(&self, _src: &mut SealedBuf) -> axio::Result<usize> {
        Err(AxError::BadFileDescriptor)
    }

    fn stat(&self) -> axio::Result<Kstat> {
        Ok(Kstat::default())
    }

    fn nonblocking(&self) -> bool {
        self.non_blocking.load(Ordering::Acquire)
    }

    fn set_nonblocking(&self, non_blocking: bool) -> axio::Result {
        self.non_blocking.store(non_blocking, Ordering::Release);
        Ok(())
    }

    fn path(&self) -> Cow<str> {
        "anon_inode:[inotify]".into()
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

impl Pollable for InotifyInstance {
    fn poll(&self) -> IoEvents {
        let mut events = IoEvents::empty();
        let queue = self.event_queue.lock();

        // Events available to read
        events.set(IoEvents::IN, !queue.is_empty());
        // inotify file is not writable
        events.set(IoEvents::OUT, false);

        events
    }

    fn register(&self, context: &mut core::task::Context<'_>, events: IoEvents) {
        if events.contains(IoEvents::IN) {
            self.poll_set.register(context.waker());
        }
    }
}

// Global manager (singleton)
struct InotifyManager {
    instances: Mutex<BTreeMap<u64, Arc<InotifyInstance>>>,
}

impl InotifyManager {
    fn new() -> Self {
        Self {
            instances: Mutex::new(BTreeMap::new()),
        }
    }

    fn register(&self, instance_id: u64, instance: Arc<InotifyInstance>) {
        self.instances.lock().insert(instance_id, instance);
    }
}

lazy_static! {
    static ref INOTIFY_MANAGER: InotifyManager = InotifyManager::new();
}
