use kmod::kbindings::*;

pub unsafe fn xa_init_flags(xa: *mut xarray, _flags: gfp_t) {
    let xa = xa.as_mut().unwrap();
    // spin_lock_init(&mut xa.xa_lock);
    xa.xa_flags = 0;
    xa.xa_head = core::ptr::null_mut();
}

pub unsafe fn xa_init(xa: *mut xarray) {
    xa_init_flags(xa, 0);
}
