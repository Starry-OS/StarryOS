use alloc::{
    borrow::Cow,
    collections::{BTreeMap, VecDeque},
    string::String,
    sync::Arc,
    vec::Vec,
};
use core::{
    any::Any,
    mem::size_of,
    sync::atomic::{AtomicBool, Ordering},
};

use axerrno::{AxError, AxResult};
use axio::{BufMut, Write};
use axpoll::{IoEvents, PollSet, Pollable};
use axsync::Mutex;
use axtask::future::{block_on, poll_io};
use bitflags::bitflags;

use crate::{
    alloc::string::ToString,
    file::{FileLike, Kstat, SealedBuf, SealedBufMut},
};

pub const IN_CLOEXEC: u32 = 0x80000;
pub const IN_NONBLOCK: u32 = 0x800;
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
    pub len: u32,    /* Size of name field
                      * attention:The name field is a variable-length array, which does not contain */
}

/// inotify instance
pub struct InotifyInstance {
    // event_queue:save serialized event data
    event_queue: Mutex<VecDeque<Vec<u8>>>,

    // watches: wd -> (path, event mask)
    watches: Mutex<BTreeMap<i32, (String, u32)>>,
    next_wd: Mutex<i32>,

    // blocking/non-blocking mode
    non_blocking: AtomicBool,

    // poll support
    poll_set: PollSet,
}

impl InotifyInstance {
    /// create new instance
    pub fn new(flags: i32) -> AxResult<Arc<Self>> {
        let flags = flags as u32;
        // verify flags
        let valid_flags = IN_NONBLOCK | IN_CLOEXEC;
        if flags & !valid_flags != 0 {
            return Err(AxError::InvalidInput);
        }

        let non_blocking = (flags & IN_NONBLOCK) != 0;

        Ok(Arc::new(Self {
            event_queue: Mutex::new(VecDeque::new()),
            watches: Mutex::new(BTreeMap::new()),
            next_wd: Mutex::new(1),
            non_blocking: AtomicBool::new(non_blocking),
            poll_set: PollSet::new(),
        }))
    }

    /// Serialized events are in binary format for users to read with char[]
    fn serialize_event(event: &InotifyEvent, name: Option<&str>) -> Vec<u8> {
        let name_len = name.map(|s| s.len()).unwrap_or(0);
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
            for _ in 0..padding {
                buf.push(0);
            }
        }

        buf
    }

    /// add watch for a path
    /// Returns watch descriptor (wd)
    pub fn add_watch(&self, path: &str, mask: u32) -> AxResult<i32> {
        let mut watches = self.watches.lock();

        // Check if a watch for this path already exists
        for (&existing_wd, (existing_path, _existing_mask)) in watches.iter() {
            if existing_path == path {
                // Overwrite existing watch (Linux default behavior)
                // Note: return the same wd
                watches.insert(existing_wd, (path.to_string(), mask));
                return Ok(existing_wd);
            }
        }

        // Generate a new watch descriptor
        let mut next_wd = self.next_wd.lock();
        let wd = *next_wd;
        *next_wd += 1;

        watches.insert(wd, (path.to_string(), mask));
        Ok(wd)
    }

    /// remove watch (generate IN_IGNORED event)
    pub fn remove_watch(&self, wd: i32) -> AxResult<()> {
        let mut watches = self.watches.lock();

        if watches.remove(&wd).is_some() {
            // Generate IN_IGNORED event (required by Linux)
            let event = InotifyEvent {
                wd,
                mask: 0x8000, // IN_IGNORED
                cookie: 0,
                len: 0,
            };

            let event_data = Self::serialize_event(&event, None);
            self.push_event(event_data);

            Ok(())
        } else {
            Err(AxError::InvalidInput)
        }
    }

    /// Push event to queue
    fn push_event(&self, event_data: Vec<u8>) {
        let mut queue = self.event_queue.lock();
        queue.push_back(event_data);
        self.poll_set.wake();
    }

    /// For testing: generate a simulated event
    pub fn generate_test_event(&self, wd: i32, mask: u32, name: Option<&str>) -> AxResult<()> {
        // Verify wd exists
        let watches = self.watches.lock();
        if !watches.contains_key(&wd) {
            return Err(AxError::InvalidInput);
        }

        let event = InotifyEvent {
            wd,
            mask,
            cookie: 0,
            len: name.map(|s| s.len() as u32).unwrap_or(0),
        };

        let event_data = Self::serialize_event(&event, name);
        self.push_event(event_data);

        Ok(())
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
