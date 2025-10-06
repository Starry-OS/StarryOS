use kernel_guard::{BaseGuard, NoPreempt};
use kspin::{SpinNoPreempt, SpinNoPreemptGuard};
pub struct KSpinNoPreempt<T>(SpinNoPreempt<T>);

impl<T> KSpinNoPreempt<T> {
    pub const fn new(data: T) -> Self {
        KSpinNoPreempt(SpinNoPreempt::new(data))
    }

    pub fn lock(&self) -> SpinNoPreemptGuard<'_, T> {
        self.0.lock()
    }

    pub fn try_lock(&self) -> Option<SpinNoPreemptGuard<'_, T>> {
        self.0.try_lock()
    }

    pub fn is_locked(&self) -> bool {
        self.0.is_locked()
    }
}

unsafe impl lock_api::RawMutex for KSpinNoPreempt<()> {
    type GuardMarker = lock_api::GuardSend;
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self = KSpinNoPreempt(SpinNoPreempt::new(()));

    fn lock(&self) {
        core::mem::forget(self.0.lock());
    }

    fn try_lock(&self) -> bool {
        // Prevent guard destructor running
        self.0.try_lock().map(core::mem::forget).is_some()
    }

    unsafe fn unlock(&self) {
        unsafe { self.0.force_unlock() };
		NoPreempt::release(());
    }

    fn is_locked(&self) -> bool {
        self.0.is_locked()
    }
}


impl From<SpinNoPreempt<()>> for KSpinNoPreempt<()> {
	fn from(spin: SpinNoPreempt<()>) -> Self {
		KSpinNoPreempt(spin)
	}
}