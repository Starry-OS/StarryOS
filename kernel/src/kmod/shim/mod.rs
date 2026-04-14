use core::ffi::{c_char, c_int, c_uint, c_void};

use kmod::{capi_fn, cdata, kbindings::*};
mod block;
mod kprint;
mod mq;
mod xarray;

#[capi_fn]
pub(super) unsafe extern "C" fn default_fault_fn() {
    axlog::error!("default_fault_fn called");
}

macro_rules! not_impl {
    ($fn_name:ident) => {
        #[capi_fn]
        unsafe extern "C" fn $fn_name() -> i32 {
            axlog::error!(concat!(stringify!($fn_name), " is not implemented"));
            0
        }
    };
}

not_impl!(__alloc_pages_noprof);
// not_impl!(__blk_mq_alloc_disk);
not_impl!(__free_pages);
// not_impl!(__kmalloc_cache_node_noprof);
// not_impl!(__kmalloc_cache_noprof);
// not_impl!(__kmalloc_noprof);

not_impl!(__mmiowb_state);

not_impl!(__per_cpu_offset);

// not_impl!(__register_blkdev);

not_impl!(__stack_chk_fail);
not_impl!(__stack_chk_guard);
not_impl!(_raw_spin_lock);
not_impl!(_raw_spin_lock_irq);
not_impl!(qspinlock_key);
not_impl!(badblocks_check);
not_impl!(badblocks_clear);
not_impl!(badblocks_exit);
not_impl!(badblocks_init);
not_impl!(badblocks_set);
not_impl!(badblocks_show);
// not_impl!(blk_mq_alloc_tag_set);
not_impl!(blk_mq_complete_request);
not_impl!(blk_mq_end_request);
not_impl!(blk_mq_end_request_batch);
not_impl!(blk_mq_free_tag_set);
// not_impl!(blk_mq_map_queues);
not_impl!(blk_mq_start_request);
not_impl!(blk_mq_start_stopped_hw_queues);
not_impl!(blk_mq_stop_hw_queues);
not_impl!(blk_mq_update_nr_hw_queues);
not_impl!(config_group_init);
not_impl!(config_group_init_type_name);
not_impl!(config_item_put);
not_impl!(configfs_register_subsystem);
not_impl!(configfs_unregister_subsystem);

not_impl!(del_gendisk);
// not_impl!(device_add_disk);
not_impl!(errno_to_blk_status);
not_impl!(hrtimer_active);
not_impl!(hrtimer_cancel);
not_impl!(hrtimer_forward);
not_impl!(hrtimer_setup);
not_impl!(hrtimer_start_range_ns);

not_impl!(hugetlb_optimize_vmemmap_key);

// not_impl!(ida_alloc_range);
not_impl!(ida_free);

not_impl!(kernel_map);
not_impl!(kfree);
not_impl!(kmalloc_caches);

// not_impl!(kstrndup);
// not_impl!(kstrtobool);
// not_impl!(kstrtoint);
// not_impl!(kstrtouint);
// not_impl!(kstrtoull);

// not_impl!(memcpy);
// not_impl!(memset);

// not_impl!(__mutex_init);
// not_impl!(mutex_lock);
// not_impl!(mutex_unlock);

// not_impl!(nr_cpu_ids);

// not_impl!(param_get_int);
// not_impl!(param_ops_bool);
// not_impl!(param_ops_int);
// not_impl!(param_ops_uint);
// not_impl!(param_ops_ulong);

not_impl!(pgtable_l4_enabled);
not_impl!(pgtable_l5_enabled);

not_impl!(put_disk);

not_impl!(radix_tree_delete_item);
not_impl!(radix_tree_gang_lookup);
not_impl!(radix_tree_insert);
not_impl!(radix_tree_lookup);
not_impl!(radix_tree_preload);

not_impl!(set_capacity);

// not_impl!(sized_strscpy);
// not_impl!(snprintf);
// not_impl!(sprintf);

// not_impl!(strchr);
// not_impl!(strcmp);
// not_impl!(strim);

not_impl!(unregister_blkdev);
not_impl!(vmemmap_start_pfn);

#[capi_fn]
unsafe extern "C" fn __kmalloc_cache_noprof(
    _s: *mut kmem_cache,
    flags: gfp_t,
    size: usize,
) -> *mut c_void {
    axlog::warn!(
        "__kmalloc_cache_noprof called with size={} gfp_t={:#x}",
        size,
        flags
    );
    let layout = match core::alloc::Layout::from_size_align(size, 8) {
        Ok(layout) => layout,
        Err(_) => {
            panic!("__kmalloc_cache_noprof: invalid layout");
        }
    };

    let ptr = unsafe { alloc::alloc::alloc(layout) };
    assert!(!ptr.is_null());
    return ptr as _;
}

#[capi_fn]
unsafe extern "C" fn __kmalloc_cache_node_noprof(
    _s: *mut kmem_cache,
    gfpflags: gfp_t,
    node: c_int,
    size: usize,
) -> *mut c_void {
    axlog::warn!(
        "__kmalloc_cache_node_noprof called with size={} gfp_t={:#x} node={}",
        size,
        gfpflags,
        node
    );
    let layout = match core::alloc::Layout::from_size_align(size, 8) {
        Ok(layout) => layout,
        Err(_) => {
            panic!("__kmalloc_cache_noprof: invalid layout");
        }
    };
    let ptr = unsafe { alloc::alloc::alloc(layout) };
    assert!(!ptr.is_null());
    return ptr as _;
}
#[capi_fn]
unsafe extern "C" fn __kmalloc_noprof(size: usize, flags: gfp_t) -> *mut c_void {
    axlog::warn!(
        "__kmalloc_noprof called with size={} gfp_t={:#x}",
        size,
        flags
    );
    let layout = match core::alloc::Layout::from_size_align(size, 8) {
        Ok(layout) => layout,
        Err(_) => {
            panic!("__kmalloc_noprof: invalid layout");
        }
    };
    let ptr = unsafe { alloc::alloc::alloc(layout) };
    assert!(!ptr.is_null());
    return ptr as _;
}

#[capi_fn]
unsafe extern "C" fn __mutex_init(_lock: *mut mutex, _name: *const c_char, _key: c_int) {
    axlog::warn!("__mutex_init called, just a stub");
}

#[capi_fn]
unsafe extern "C" fn mutex_lock(_lock: *mut mutex) {
    axlog::warn!("mutex_lock called, just a stub");
}

#[capi_fn]
unsafe extern "C" fn mutex_unlock(_lock: *mut mutex) {
    axlog::warn!("mutex_unlock called, just a stub");
}

// not_impl!(__mutex_init);
// not_impl!(mutex_lock);
// not_impl!(mutex_unlock);

#[cdata]
/// CPU count
pub static nr_cpu_ids: c_int = 1;

#[inline(always)]
fn init_list_head(list: &mut list_head) {
    list.next = list as *mut _;
    list.prev = list as *mut _;
}

#[capi_fn]
unsafe extern "C" fn ida_alloc_range(
    _arg1: *mut ida,
    min: c_uint,
    max: c_uint,
    arg2: gfp_t,
) -> c_int {
    axlog::warn!(
        "ida_alloc_range called with min={} max={} gfp_t={:#x}",
        min,
        max,
        arg2
    );
    // let ida = arg1.as_mut().unwrap();
    static mut COUNTER: c_uint = 0;
    if COUNTER < max {
        COUNTER += 1;
        return (COUNTER - 1) as c_int;
    } else {
        axlog::error!("ida_alloc_range: no more IDs available");
        return -1;
    }
}
