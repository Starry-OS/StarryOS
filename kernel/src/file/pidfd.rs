use alloc::{
    borrow::Cow,
    sync::{Arc, Weak},
};
use core::{
    sync::atomic::{AtomicBool, Ordering},
    task::Context,
};

use axerrno::{AxError, AxResult};
use axpoll::{IoEvents, PollSet, Pollable};

use crate::{
    file::FileLike,
    task::{ProcessData, Thread},
};

pub struct PidFd {
    proc_data: Weak<ProcessData>,
    exit_event: Weak<PollSet>,

    non_blocking: AtomicBool,
}
impl PidFd {
    pub fn new_process(proc_data: &Arc<ProcessData>) -> Self {
        Self {
            proc_data: Arc::downgrade(proc_data),
            exit_event: Arc::downgrade(&proc_data.exit_event),

            non_blocking: AtomicBool::new(false),
        }
    }

    pub fn new_thread(thread: &Thread) -> Self {
        Self {
            proc_data: Arc::downgrade(&thread.proc_data),
            exit_event: Arc::downgrade(&thread.exit_event),

            non_blocking: AtomicBool::new(false),
        }
    }

    pub fn process_data(&self) -> AxResult<Arc<ProcessData>> {
        // For threads, the pidfd is invalid once the thread exits, even if its
        // process is still alive.
        if self.exit_event.strong_count() == 0 {
            return Err(AxError::NoSuchProcess);
        }
        self.proc_data.upgrade().ok_or(AxError::NoSuchProcess)
    }
}
impl FileLike for PidFd {
    fn path(&self) -> Cow<'_, str> {
        "anon_inode:[pidfd]".into()
    }

    fn set_nonblocking(&self, nonblocking: bool) -> AxResult {
        self.non_blocking.store(nonblocking, Ordering::Release);
        Ok(())
    }

    fn nonblocking(&self) -> bool {
        self.non_blocking.load(Ordering::Acquire)
    }
}

impl Pollable for PidFd {
    fn poll(&self) -> IoEvents {
        let mut events = IoEvents::empty();
        events.set(IoEvents::IN, self.exit_event.strong_count() > 0);
        events
    }

    fn register(&self, context: &mut Context<'_>, events: IoEvents) {
        if events.contains(IoEvents::IN)
            && let Some(exit_event) = self.exit_event.upgrade()
        {
            exit_event.register(context.waker());
        }
    }
}
