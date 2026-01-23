use core::{
    alloc::Layout,
    ffi::{c_int, c_void},
    sync::atomic::AtomicU32,
};

use axalloc::UsageKind;
use axerrno::LinuxError;
use axhal::{mem::phys_to_virt, paging::PageSize};
use axmm::backend::alloc_frames;
use kmod::{capi_fn, kbindings::*};
use memory_addr::PAGE_SIZE_4K;

use super::{nr_cpu_ids, xarray::*};

const BLK_MQ_TAG_MIN: u32 = 1;

// Alloc a tag set to be associated with one or more request queues.
// May fail with EINVAL for various error conditions. May adjust the
// requested depth down, if it's too large. In that case, the set
// value will be stored in set->queue_depth.
#[capi_fn]
unsafe extern "C" fn blk_mq_alloc_tag_set(set: *mut blk_mq_tag_set) -> c_int {
    axlog::error!("[blk_mq_alloc_tag_set] is called");
    let mq_tag_set = set.as_mut().unwrap();
    if mq_tag_set.nr_hw_queues == 0 {
        axlog::error!("blk_mq_alloc_tag_set: nr_hw_queues is zero");
        return -(LinuxError::EINVAL as c_int);
    }
    if mq_tag_set.queue_depth == 0 {
        axlog::error!("blk_mq_alloc_tag_set: queue_depth is zero");
        return -(LinuxError::EINVAL as c_int);
    }
    if mq_tag_set.queue_depth < mq_tag_set.reserved_tags + BLK_MQ_TAG_MIN {
        axlog::error!(
            "blk_mq_alloc_tag_set: queue_depth is less than reserved_tags + BLK_MQ_TAG_MIN"
        );
        return -(LinuxError::EINVAL as c_int);
    }

    axlog::warn!(
        "blk_mq_alloc_tag_set: nr_hw_queues: {}, queue_depth: {}, reserved_tags: {}, nr_maps: {}",
        mq_tag_set.nr_hw_queues,
        mq_tag_set.queue_depth,
        mq_tag_set.reserved_tags,
        mq_tag_set.nr_maps,
    );

    let ops = mq_tag_set.ops.as_ref().unwrap();
    if ops.queue_rq.is_none() {
        axlog::error!("blk_mq_alloc_tag_set: ops.queue_rq is None");
        return -(LinuxError::EINVAL as c_int);
    }
    if ops.get_budget.is_none() ^ ops.put_budget.is_none() {
        axlog::error!(
            "blk_mq_alloc_tag_set: ops.get_budget and ops.put_budget must be both set or both None"
        );
        return -(LinuxError::EINVAL as c_int);
    }
    if mq_tag_set.queue_depth > BLK_MQ_MAX_DEPTH as u32 {
        axlog::warn!("blk-mq: reduced tag depth to {}", BLK_MQ_MAX_DEPTH);
        mq_tag_set.queue_depth = BLK_MQ_MAX_DEPTH as u32;
    }
    if mq_tag_set.nr_maps == 0 {
        mq_tag_set.nr_maps = 1;
    } else if mq_tag_set.nr_maps > hctx_type_HCTX_MAX_TYPES as u32 {
        axlog::error!("blk_mq_alloc_tag_set: nr_maps is greater than HCTX_MAX_TYPES");
        return -(LinuxError::EINVAL as c_int);
    }

    // There is no use for more h/w queues than cpus if we just have a single map
    if mq_tag_set.nr_maps == 1 && mq_tag_set.nr_hw_queues > nr_cpu_ids as u32 {
        mq_tag_set.nr_hw_queues = nr_cpu_ids as u32;
    }

    if (mq_tag_set.flags & BLK_MQ_F_BLOCKING) != 0 {
        // Allocate and initialize srcu struct
        // For simplicity, we skip actual allocation in this shim
        // In a real implementation, you would allocate memory here
        // and handle errors accordingly
        unimplemented!("BLK_MQ_F_BLOCKING is not implemented in this shim");
    }

    // init_rwsem(&set->update_nr_hwq_lock);

    let layout = Layout::from_size_align(
        (mq_tag_set.nr_hw_queues as usize) * core::mem::size_of::<*mut blk_mq_tags>(),
        core::mem::align_of::<*mut blk_mq_tags>(),
    )
    .unwrap();

    let tags_ptr = alloc::alloc::alloc(layout) as *mut *mut blk_mq_tags;
    assert!(
        !tags_ptr.is_null(),
        "Failed to allocate memory for blk_mq_tag_set tags"
    );
    mq_tag_set.tags = tags_ptr;

    for i in 0..mq_tag_set.nr_maps {
        let map = &mut mq_tag_set.map[i as usize];
        let map_layout = Layout::from_size_align(
            (nr_cpu_ids as usize) * core::mem::size_of_val_raw(map.mq_map),
            core::mem::align_of::<blk_mq_queue_map>(),
        )
        .unwrap();
        let mq_map_ptr = alloc::alloc::alloc(map_layout) as *mut u32;
        assert!(
            !mq_map_ptr.is_null(),
            "Failed to allocate memory for blk_mq_queue_map mq_map"
        );
        map.mq_map = mq_map_ptr;
        map.nr_queues = mq_tag_set.nr_hw_queues;
    }

    blk_mq_update_queue_map(mq_tag_set);

    let ret = blk_mq_alloc_set_map_and_rqs(mq_tag_set);
    assert!(
        ret == 0,
        "blk_mq_alloc_tag_set: blk_mq_alloc_set_map_and_rqs failed"
    );

    // mutex_init(&set->tag_list_lock);
    super::init_list_head(&mut mq_tag_set.tag_list);

    0
}

// Allocate the request maps associated with this tag_set. Note that this
// may reduce the depth asked for, if memory is tight. set->queue_depth
// will be updated to reflect the allocated depth.
unsafe fn blk_mq_alloc_set_map_and_rqs(set: &mut blk_mq_tag_set) -> c_int {
    axlog::warn!("[blk_mq_alloc_set_map_and_rqs] is called");
    let depth = set.queue_depth;
    loop {
        let err = __blk_mq_alloc_rq_maps(set);
        if err == 0 {
            break;
        }
        set.queue_depth >>= 1;
        if set.queue_depth < set.reserved_tags + BLK_MQ_TAG_MIN {
            return -(LinuxError::ENOMEM as c_int);
        }
        if set.queue_depth == 0 {
            break;
        }
    }
    if set.queue_depth == 0 {
        axlog::error!("blk-mq: failed to allocate request map");
        return -(LinuxError::ENOMEM as c_int);
    }
    if depth != set.queue_depth {
        axlog::warn!(
            "blk-mq: reduced tag depth ({} -> {})",
            depth,
            set.queue_depth
        );
    }
    0
}

fn blk_mq_is_shared_tags(flags: u32) -> bool {
    (flags & BLK_MQ_F_TAG_HCTX_SHARED) != 0
}

unsafe fn __blk_mq_alloc_rq_maps(set: &mut blk_mq_tag_set) -> c_int {
    if blk_mq_is_shared_tags(set.flags) {
        axlog::warn!("blk_mq_is_shared_tags: allocating shared tags");
        set.shared_tags = blk_mq_alloc_map_and_rqs(set, BLK_MQ_NO_HCTX_IDX as _, set.queue_depth);
    }

    axlog::warn!(
        "blk_mq_alloc_rq_maps: Allocating rq maps for {} hw_queues",
        set.nr_hw_queues
    );
    for i in 0..set.nr_hw_queues {
        let ret = __blk_mq_alloc_map_and_rqs(set, i as c_int);
        assert!(
            ret,
            "blk_mq_alloc_rq_maps: __blk_mq_alloc_map_and_rqs failed"
        );
    }
    0
}

unsafe fn __blk_mq_alloc_map_and_rqs(set: &mut blk_mq_tag_set, hctx_idx: c_int) -> bool {
    axlog::warn!(
        "[__blk_mq_alloc_map_and_rqs] is called with hctx_idx: {}",
        hctx_idx
    );
    if blk_mq_is_shared_tags(set.flags) {
        *set.tags.add(hctx_idx as usize) = set.shared_tags;
        return true;
    }
    *set.tags.add(hctx_idx as usize) =
        blk_mq_alloc_map_and_rqs(set, hctx_idx as u32, set.queue_depth);
    return !(*set.tags.add(hctx_idx as usize)).is_null();
}

unsafe fn blk_mq_alloc_map_and_rqs(
    set: &mut blk_mq_tag_set,
    hctx_idx: u32,
    depth: u32,
) -> *mut blk_mq_tags {
    axlog::warn!(
        "[blk_mq_alloc_map_and_rqs]: hctx_idx: {}, depth: {}",
        hctx_idx,
        depth
    );
    let tags = blk_mq_alloc_rq_map(set, hctx_idx, depth, set.reserved_tags);
    assert!(
        !tags.is_null(),
        "[blk_mq_alloc_map_and_rqs]: blk_mq_alloc_rq_map failed"
    );

    let ret = blk_mq_alloc_rqs(set, tags.as_mut().unwrap(), hctx_idx, depth);
    assert!(
        ret == 0,
        "[blk_mq_alloc_map_and_rqs]: blk_mq_alloc_rqs failed"
    );
    tags
}

unsafe fn blk_mq_alloc_rqs(
    set: &mut blk_mq_tag_set,
    tags: &mut blk_mq_tags,
    _hctx_idx: u32,
    depth: u32,
) -> c_int {
    let _node = set.numa_node;
    super::init_list_head(&mut tags.page_list);
    // rq_size is the size of the request plus driver payload, rounded
    // to the cacheline size
    let rq_size = size_of::<request>() + set.cmd_size as usize;
    let rq_size_rounded = (rq_size + 63) & !63; // round up to cache line size
    let left = rq_size_rounded * depth as usize;

    let nums = (left + PAGE_SIZE_4K - 1) / PAGE_SIZE_4K;
    let pages = alloc_frames(true, PageSize::Size4K, nums, UsageKind::Global).unwrap();

    axlog::warn!(
        "blk_mq_alloc_rqs: Allocated {} pages for {} requests, rq_size_rounded: {}, total_size: {}",
        nums,
        depth,
        rq_size_rounded,
        left
    );

    let addr = phys_to_virt(pages).as_mut_ptr();
    for i in 0..depth {
        let rq_ptr = unsafe { addr.add(i as usize * rq_size_rounded) } as *mut request;
        blk_mq_init_request(set, rq_ptr, _hctx_idx, _node);
        unsafe {
            tags.static_rqs.add(i as usize).write(rq_ptr);
        }
    }
    0
}

unsafe fn blk_mq_init_request(
    set: &mut blk_mq_tag_set,
    rq: *mut request,
    hctx_idx: u32,
    node: c_int,
) -> c_int {
    let ops = set.ops.as_ref().unwrap();
    if let Some(init_request_fn) = ops.init_request {
        let ret = init_request_fn(set, rq, hctx_idx, node as _);
        assert!(ret == 0, "blk_mq_init_request: init_request_fn failed");
    }
    unsafe {
        core::ptr::write_volatile(&mut (*rq).state, MQ_RQ_IDLE);
    }
    0
}

pub const MQ_RQ_IDLE: u32 = 0;

unsafe fn blk_mq_alloc_rq_map(
    set: &blk_mq_tag_set,
    hctx_idx: u32,
    nr_tags: u32,
    reserved_tags: u32,
) -> *mut blk_mq_tags {
    axlog::error!(
        "[blk_mq_alloc_rq_map] is called with hctx_idx: {}, nr_tags: {}, reserved_tags: {}",
        hctx_idx,
        nr_tags,
        reserved_tags
    );
    let node = set.numa_node;

    let tags = blk_mq_init_tags(nr_tags, reserved_tags, set.flags, node);
    assert!(
        !tags.is_null(),
        "blk_mq_alloc_rq_map: blk_mq_init_tags failed"
    );

    // Allocate rqs and static_rqs arrays
    let layout_rqs = Layout::from_size_align(
        (nr_tags as usize) * core::mem::size_of::<*mut request>(),
        core::mem::align_of::<*mut request>(),
    )
    .unwrap();
    let rqs_ptr = alloc::alloc::alloc(layout_rqs) as *mut *mut request;
    assert!(
        !rqs_ptr.is_null(),
        "blk_mq_alloc_rq_map: failed to allocate rqs array"
    );
    (*tags).rqs = rqs_ptr;

    let static_rqs_ptr = alloc::alloc::alloc(layout_rqs) as *mut *mut request;
    assert!(
        !static_rqs_ptr.is_null(),
        "blk_mq_alloc_rq_map: failed to allocate static_rqs array"
    );
    (*tags).static_rqs = static_rqs_ptr;
    axlog::warn!("blk_mq_alloc_rq_map: Allocated request array for tags.rqs and tags.static_rqs");
    tags
}

unsafe fn blk_mq_init_tags(
    total_tags: u32,
    reserved_tags: u32,
    flags: u32,
    _node: c_int,
) -> *mut blk_mq_tags {
    let _depth = total_tags - reserved_tags;
    let _round_robin = (flags & BLK_MQ_F_TAG_RR) != 0;

    axlog::warn!("[blk_mq_init_tags] total_tags: {}", total_tags);

    let layout =
        Layout::from_size_align(size_of::<blk_mq_tags>(), align_of::<blk_mq_tags>()).unwrap();
    let tags_ptr = alloc::alloc::alloc(layout) as *mut blk_mq_tags;
    assert!(
        !tags_ptr.is_null(),
        "blk_mq_init_tags: failed to allocate blk_mq_tags"
    );
    let tags = &mut *tags_ptr;
    tags.nr_tags = total_tags;
    tags.nr_reserved_tags = reserved_tags;
    // spin_lock_init(&tags->lock);
    // bt_alloc
    tags_ptr
}

fn bt_alloc() {
    axlog::warn!("[bt_alloc] is not implemented");
}

unsafe fn blk_mq_update_queue_map(set: &mut blk_mq_tag_set) {
    // blk_mq_map_queues() and multiple .map_queues() implementations
    // expect that set->map[HCTX_TYPE_DEFAULT].nr_queues is set to the
    // number of hardware queues.
    if set.nr_maps == 1 {
        set.map[hctx_type_HCTX_TYPE_DEFAULT as usize].nr_queues = set.nr_hw_queues;
    }

    let ops = set.ops.as_ref().unwrap();
    if let Some(map_queues_fn) = ops.map_queues {
        // transport .map_queues is usually done in the following
        // way:
        //
        // for (queue = 0; queue < set->nr_hw_queues; queue++) {
        // 	mask = get_cpu_mask(queue)
        // 	for_each_cpu(cpu, mask)
        // 		set->map[x].mq_map[cpu] = queue;
        // }
        //
        // When we need to remap, the table has to be cleared for
        // killing stale mapping since one CPU may not be mapped
        // to any hw queue.
        for i in 0..set.nr_maps {
            blk_mq_clear_mq_map(&mut set.map[i as usize]);
        }

        map_queues_fn(set);
    } else {
        assert!(
            set.nr_maps <= 1,
            "blk_mq_update_queue_map: nr_maps > 1 without map_queues"
        );
        blk_mq_map_queues(&mut set.map[hctx_type_HCTX_TYPE_DEFAULT as usize]);
    }
}

unsafe fn blk_mq_clear_mq_map(qmap: &mut blk_mq_queue_map) {
    axlog::warn!("[blk_mq_clear_mq_map] is called");
    for cpu in 0..nr_cpu_ids {
        qmap.mq_map.add(cpu as usize).write(0);
    }
}

#[capi_fn]
unsafe extern "C" fn blk_mq_map_queues(qmap: *mut blk_mq_queue_map) {
    let qmap = unsafe { &mut *qmap };
    // assert!(
    //     nr_cpu_ids == 0 && qmap.nr_queues == 1,
    //     "blk_mq_map_queues: only single CPU and single queue supported in this
    // shim" );
    axlog::warn!("[blk_mq_map_queues] qmap.nr_queues: {}", qmap.nr_queues);
    for cpu in 0..nr_cpu_ids {
        let queue = (cpu as u32) % qmap.nr_queues;
        axlog::warn!("blk_mq_map_queues: mapping cpu {} to queue {}", cpu, queue);
        unsafe {
            qmap.mq_map.add(cpu as usize).write(queue);
        }
    }
}

#[capi_fn]
unsafe extern "C" fn __blk_mq_alloc_disk(
    set: *mut blk_mq_tag_set,
    lim: *mut queue_limits,
    queuedata: *mut c_void,
    lkclass: *mut lock_class_key,
) -> *mut gendisk {
    axlog::warn!("[__blk_mq_alloc_disk] is called");
    let q = blk_mq_alloc_queue(set, lim, queuedata);
    assert!(
        !q.is_null(),
        "__blk_mq_alloc_disk: blk_mq_alloc_queue failed"
    );

    let set = set.as_mut().unwrap();
    let disk = __alloc_disk_node(q, set.numa_node, lkclass);

    let disk_mut = disk.as_mut().unwrap();
    disk_mut.state |= 1 << GD_OWNS_QUEUE;

    return disk;
}

unsafe fn __alloc_disk_node(
    q: *mut request_queue,
    node_id: c_int,
    _lkclass: *mut lock_class_key,
) -> *mut gendisk {
    axlog::warn!("[__alloc_disk_node] is called");
    let layout = Layout::from_size_align(
        core::mem::size_of::<gendisk>(),
        core::mem::align_of::<gendisk>(),
    )
    .unwrap();
    let disk_ptr = alloc::alloc::alloc(layout) as *mut gendisk;
    assert!(
        !disk_ptr.is_null(),
        "__alloc_disk_node: failed to allocate gendisk"
    );

    let disk = disk_ptr.as_mut().unwrap();
    disk.queue = q;

    // alloc block device structures like part0
    // disk->part0 = bdev_alloc(disk, 0);

    disk.node_id = node_id;

    // disk_init_zone_resources(disk);
    // rand_initialize_disk(disk);

    let q = q.as_mut().unwrap();

    q.disk = disk;

    disk_ptr
}

const BLK_FEAT_IO_STAT: u32 = 1 << 4;
const BLK_FEAT_NOWAIT: u32 = 1 << 7;
const BLK_FEAT_POLL: u32 = 1 << 9;

#[capi_fn]
unsafe extern "C" fn blk_mq_alloc_queue(
    set: *mut blk_mq_tag_set,
    lim: *mut queue_limits,
    queuedata: *mut c_void,
) -> *mut request_queue {
    let mut default_lim = queue_limits::default();

    let lim = if lim.is_null() {
        &mut default_lim as *mut queue_limits
    } else {
        lim
    };
    let lim_ref = unsafe { &mut *lim };
    lim_ref.features |= BLK_FEAT_IO_STAT | BLK_FEAT_NOWAIT;

    let set = set.as_mut().unwrap();
    if set.nr_maps > hctx_type_HCTX_TYPE_POLL as u32 {
        lim_ref.features |= BLK_FEAT_POLL;
    }

    let q = blk_alloc_queue(lim_ref, set.numa_node);
    assert!(!q.is_null(), "blk_mq_alloc_queue: blk_alloc_queue failed");

    (*q).queuedata = queuedata;

    let ret = blk_mq_init_allocated_queue(set, q);
    assert!(
        ret == 0,
        "blk_mq_alloc_queue: blk_mq_init_allocated_queue failed"
    );

    q
}

static BLK_ID: AtomicU32 = AtomicU32::new(1);

#[capi_fn]
unsafe extern "C" fn blk_alloc_queue(lim: *mut queue_limits, node_id: c_int) -> *mut request_queue {
    axlog::warn!("[blk_alloc_queue] is called");
    let layout = Layout::from_size_align(
        core::mem::size_of::<request_queue>(),
        core::mem::align_of::<request_queue>(),
    )
    .unwrap();
    let q_ptr = alloc::alloc::alloc(layout) as *mut request_queue;
    assert!(
        !q_ptr.is_null(),
        "blk_alloc_queue: failed to allocate request_queue"
    );

    let req_q = q_ptr.as_mut().unwrap();
    req_q.last_merge = core::ptr::null_mut();
    req_q.id = BLK_ID.fetch_add(1, core::sync::atomic::Ordering::SeqCst) as c_int;

    req_q.stats = blk_alloc_queue_stats();

    let error = blk_set_default_limits(&mut *lim);
    assert!(error == 0, "blk_alloc_queue: blk_set_default_limits failed");

    req_q.limits = *lim;
    req_q.node = node_id;

    req_q.nr_active_requests_shared_tags = atomic_t { counter: 0 };

    // timer_setup
    axlog::warn!("blk_alloc_queue: timer_setup is not implemented in this shim");
    // INIT_WORK
    axlog::warn!("blk_alloc_queue: INIT_WORK is not implemented in this shim");
    // INIT_LIST_HEAD
    super::init_list_head(&mut req_q.icq_list);
    req_q.refs = refcount_t {
        refs: atomic_t { counter: 1 },
    };

    // mutex_init(&q->debugfs_mutex);
    // mutex_init(&q->elevator_lock);
    // mutex_init(&q->sysfs_lock);
    // mutex_init(&q->limits_lock);
    // mutex_init(&q->rq_qos_mutex);
    // spin_lock_init(&q->queue_lock);
    // init_waitqueue_head(&q->mq_freeze_wq);
    // mutex_init(&q->mq_freeze_lock);

    blkg_init_queue(req_q);

    // Init percpu_ref in atomic mode so that it's faster to shutdown.
    // See blk_register_queue() for details.
    // percpu_ref_init
    axlog::warn!("blk_alloc_queue: percpu_ref_init is not implemented in this shim");
    // lockdep_register_key(&mut req_q.io_lock_cls_key);
    // lockdep_register_key(&mut req_q.q_lock_cls_key);

    req_q.nr_requests = BLKDEV_DEFAULT_RQ;
    q_ptr
}

const BLKDEV_DEFAULT_RQ: u64 = 128;

fn blkg_init_queue(req_q: &mut request_queue) {
    axlog::warn!("[blkg_init_queue] is called");
    super::init_list_head(&mut req_q.blkg_list);
    // mutex_init(&q->blkcg_mutex);
}

unsafe fn blk_alloc_queue_stats() -> *mut blk_queue_stats {
    axlog::warn!("[blk_alloc_queue_stats] is called");
    let layout = Layout::from_size_align(
        core::mem::size_of::<blk_queue_stats>(),
        core::mem::align_of::<blk_queue_stats>(),
    )
    .unwrap();
    let stats_ptr = unsafe { alloc::alloc::alloc(layout) as *mut blk_queue_stats };
    assert!(
        !stats_ptr.is_null(),
        "blk_alloc_queue_stats: failed to allocate blk_queue_stats"
    );
    // let stats = stats_ptr.as_mut().unwrap();
    // super::init_list_head(&mut stats.callbacks);
    // spin_lock_init(&stats.lock);
    // stats.accounting = 0;
    stats_ptr
}

// Set the default limits for a newly allocated queue.  @lim contains the
// initial limits set by the driver, which could be no limit in which case
// all fields are cleared to zero.
fn blk_set_default_limits(lim: &mut queue_limits) -> c_int {
    // Most defaults are set by capping the bounds in blk_validate_limits,
    // but these limits are special and need an explicit initialization to
    // the max value here.
    lim.max_user_discard_sectors = u32::MAX;
    lim.max_user_wzeroes_unmap_sectors = u32::MAX;
    blk_validate_limits(lim)
}

fn blk_validate_block_size(bsize: u64) -> bool {
    if bsize < 512 || bsize > PAGE_SIZE as u64 || !bsize.is_power_of_two() {
        return true;
    }
    false
}

fn round_down(value: u32, align: u32) -> u32 {
    value & !(align - 1)
}

// Check that the limits in lim are valid, initialize defaults for unset
// values, and cap values based on others where needed.
fn blk_validate_limits(lim: &mut queue_limits) -> c_int {
    // Unless otherwise specified, default to 512 byte logical blocks and a
    // physical block size equal to the logical block size.
    if lim.logical_block_size == 0 {
        lim.logical_block_size = 512;
    } else if blk_validate_block_size(lim.logical_block_size as u64) {
        axlog::error!("Invalid logical block size ({})", lim.logical_block_size);
        return -(LinuxError::EINVAL as c_int);
    }

    if lim.physical_block_size < lim.logical_block_size {
        lim.physical_block_size = lim.logical_block_size;
    } else if !lim.physical_block_size.is_power_of_two() {
        axlog::error!("Invalid physical block size ({})", lim.physical_block_size);
        return -(LinuxError::EINVAL as c_int);
    }
    // The minimum I/O size defaults to the physical block size unless
    // explicitly overridden.
    if lim.io_min < lim.physical_block_size {
        lim.io_min = lim.physical_block_size;
    }

    // The optimal I/O size may not be aligned to physical block size
    // (because it may be limited by dma engines which have no clue about
    // block size of the disks attached to them), so we round it down here.
    lim.io_opt = round_down(lim.io_opt, lim.physical_block_size);

    // max_hw_sectors has a somewhat weird default for historical reason,
    // but driver really should set their own instead of relying on this
    // value.
    //
    // The block layer relies on the fact that every driver can
    // handle at lest a page worth of data per I/O, and needs the value
    // aligned to the logical block size.
    

    if lim.max_hw_sectors == 0 {
        lim.max_hw_sectors = BLK_SAFE_MAX_SECTORS; // BLK_SAFE_MAX_SECTORS
    }

    if lim.max_hw_sectors < PAGE_SECTORS as u32 {
        axlog::error!("blk_validate_limits: max_hw_sectors is less than PAGE_SECTORS");
        return -(LinuxError::EINVAL as c_int);
    }
    let logical_block_sectors = lim.logical_block_size >> SECTOR_SHIFT;
    if logical_block_sectors > lim.max_hw_sectors {
        axlog::error!("blk_validate_limits: logical_block_sectors is greater than max_hw_sectors");
        return -(LinuxError::EINVAL as c_int);
    }
    lim.max_hw_sectors = round_down(lim.max_hw_sectors, logical_block_sectors as u32);

    // The actual max_sectors value is a complex beast and also takes the
    // max_dev_sectors value (set by SCSI ULPs) and a user configurable
    // value into account.  The ->max_sectors value is always calculated
    // from these, so directly setting it won't have any effect.
    let max_hw_sectors = min_not_zero(lim.max_hw_sectors, lim.max_dev_sectors);
    if lim.max_user_sectors != 0 {
        if lim.max_user_sectors < (BLK_MIN_SEGMENT_SIZE as u32 / SECTOR_SIZE as u32) {
            axlog::error!(
                "blk_validate_limits: max_user_sectors is less than BLK_MIN_SEGMENT_SIZE"
            );
            return -(LinuxError::EINVAL as c_int);
        }
        lim.max_sectors = min_not_zero(max_hw_sectors, lim.max_user_sectors);
    } else if lim.io_opt > ((BLK_DEF_MAX_SECTORS_CAP as u32) << SECTOR_SHIFT) {
        lim.max_sectors = min_not_zero(max_hw_sectors, lim.io_opt >> SECTOR_SHIFT);
    } else if lim.io_min > ((BLK_DEF_MAX_SECTORS_CAP as u32) << SECTOR_SHIFT) {
        lim.max_sectors = min_not_zero(max_hw_sectors, lim.io_min >> SECTOR_SHIFT);
    } else {
        lim.max_sectors = min_not_zero(max_hw_sectors, BLK_DEF_MAX_SECTORS_CAP as u32);
    }
    lim.max_sectors = round_down(lim.max_sectors, logical_block_sectors as u32);

    // Random default for the maximum number of segments.  Driver should not
    // rely on this and set their own.
    if lim.max_segments <= 0 {
        lim.max_segments = BLK_MAX_SEGMENTS as _;
    }
    if lim.max_hw_wzeroes_unmap_sectors > 0
        && lim.max_hw_wzeroes_unmap_sectors != lim.max_write_zeroes_sectors
    {
        axlog::error!("blk_validate_limits: max_hw_wzeroes_unmap_sectors is inconsistent");
        return -(LinuxError::EINVAL as c_int);
    }
    lim.max_wzeroes_unmap_sectors = core::cmp::min(
        lim.max_hw_wzeroes_unmap_sectors,
        lim.max_user_wzeroes_unmap_sectors,
    );
    lim.max_discard_sectors =
        core::cmp::min(lim.max_hw_discard_sectors, lim.max_user_discard_sectors);

    // When discard is not supported, discard_granularity should be reported
    // as 0 to userspace.
    if lim.max_discard_sectors > 0 {
        lim.discard_granularity = core::cmp::max(lim.discard_granularity, lim.physical_block_size);
    } else {
        lim.discard_granularity = 0;
    }

    if lim.max_discard_segments <= 0 {
        lim.max_discard_segments = 1;
    }

    if lim.seg_boundary_mask <= 0 {
        lim.seg_boundary_mask = BLK_SEG_BOUNDARY_MASK as _;
    }
    if lim.seg_boundary_mask < (BLK_MIN_SEGMENT_SIZE - 1) as _ {
        axlog::error!(
            "blk_validate_limits: seg_boundary_mask is less than BLK_MIN_SEGMENT_SIZE - 1"
        );
        return -(LinuxError::EINVAL as c_int);
    }

    // Stacking device may have both virtual boundary and max segment
    // size limit, so allow this setting now, and long-term the two
    // might need to move out of stacking limits since we have immutable
    // bvec and lower layer bio splitting is supposed to handle the two
    // correctly.
    if lim.virt_boundary_mask > 0 {
        if lim.max_segment_size <= 0 {
            lim.max_segment_size = u32::MAX;
        }
    } else {
        // The maximum segment size has an odd historic 64k default that
        // drivers probably should override.  Just like the I/O size we
        // require drivers to at least handle a full page per segment.
        if lim.max_segment_size == 0 {
            lim.max_segment_size = BLK_MAX_SEGMENT_SIZE;
        }
        if lim.max_segment_size < BLK_MIN_SEGMENT_SIZE as _ {
            axlog::error!(
                "blk_validate_limits: max_segment_size is less than BLK_MIN_SEGMENT_SIZE"
            );
            return -(LinuxError::EINVAL as c_int);
        }
    }

    // setup min segment size for building new segment in fast path
    let seg_size = if lim.seg_boundary_mask > (lim.max_segment_size - 1) as _ {
        lim.max_segment_size as u64
    } else {
        (lim.seg_boundary_mask + 1) as u64
    };
    lim.min_segment_size = core::cmp::min(seg_size as u32, PAGE_SIZE as u32);

    // We require drivers to at least do logical block aligned I/O, but
    // historically could not check for that due to the separate calls
    // to set the limits.  Once the transition is finished the check
    // below should be narrowed down to check the logical block size.
    if lim.dma_alignment <= 0 {
        lim.dma_alignment = SECTOR_SIZE - 1;
    }

    if lim.dma_alignment > PAGE_SIZE as _ {
        axlog::error!("blk_validate_limits: dma_alignment is greater than PAGE_SIZE");
        return -(LinuxError::EINVAL as c_int);
    }

    if lim.alignment_offset > 0 {
        lim.alignment_offset &= lim.physical_block_size - 1;
        lim.flags &= !BLK_FLAG_MISALIGNED;
    }

    if lim.features & BLK_FEAT_WRITE_CACHE == 0 {
        lim.features &= !BLK_FEAT_FUA;
    }

    // blk_validate_atomic_write_limits
    // blk_validate_integrity_limits
    // blk_validate_zoned_limits

    axlog::warn!("blk_validate_limits: skip blk_validate_atomic_write_limits");
    axlog::warn!("blk_validate_limits: skip blk_validate_integrity_limits");
    axlog::warn!("blk_validate_limits: skip blk_validate_zoned_limits");
    0
}

const BLK_FEAT_WRITE_CACHE: u32 = 1;
const BLK_FEAT_FUA: u32 = 2;
const BLK_FLAG_MISALIGNED: u32 = 1 << 1;
const BLK_SAFE_MAX_SECTORS: u32 = 255;
const BLK_MIN_SEGMENT_SIZE: usize = 4096;
const BLK_DEF_MAX_SECTORS_CAP: usize = 8192;
const BLK_MAX_SEGMENTS: u32 = 128;
const BLK_MAX_SEGMENT_SIZE: u32 = 65536;

const BLK_SEG_BOUNDARY_MASK: u32 = 0xFFFFFFFF;
fn min_not_zero(a: u32, b: u32) -> u32 {
    match (a, b) {
        (0, 0) => 0,
        (0, _) => b,
        (_, 0) => a,
        _ => core::cmp::min(a, b),
    }
}

#[capi_fn]
unsafe extern "C" fn blk_mq_init_allocated_queue(
    set: *mut blk_mq_tag_set,
    q: *mut request_queue,
) -> c_int {
    axlog::warn!("[blk_mq_init_allocated_queue] is called");
    let set = unsafe { &mut *set };
    let q = unsafe { &mut *q };
    // mark the queue as mq asap
    q.mq_ops = set.ops;

    // ->tag_set has to be setup before initialize hctx, which cpuphp
    // handler needs it for checking queue mapping
    q.tag_set = set;

    let ret = blk_mq_alloc_ctxs(q);
    assert!(
        ret == 0,
        "blk_mq_init_allocated_queue: blk_mq_alloc_ctxs failed"
    );

    // init q->mq_kobj and sw queues' kobjects
    // blk_mq_sysfs_init(q);

    super::init_list_head(&mut q.unused_hctx_list);
    // spin_lock_init(&q->unused_hctx_lock);

    xa_init(&mut q.hctx_table);

    blk_mq_realloc_hw_ctxs(set, q);

    assert!(
        q.nr_hw_queues == set.nr_hw_queues,
        "blk_mq_init_allocated_queue: q.nr_hw_queues != set.nr_hw_queues"
    );

    q.queue_flags |= QUEUE_FLAG_MQ_DEFAULT as u64;

    super::init_list_head(&mut q.flush_list);
    super::init_list_head(&mut q.requeue_list);

    q.nr_requests = set.queue_depth as _;

    // blk_mq_init_cpu_queues(q, set.nr_hw_queues);

    // blk_mq_map_swqueue(q);

    // blk_mq_add_queue_tag_set(q);

    0
}

const QUEUE_FLAG_MQ_DEFAULT: u32 = 4;

unsafe fn blk_mq_realloc_hw_ctxs(set: &mut blk_mq_tag_set, q: &mut request_queue) {
    axlog::warn!("[blk_mq_realloc_hw_ctxs] is called");
    __blk_mq_realloc_hw_ctxs(set, q);
    // unregister cpuhp callbacks for exited hctxs
    // blk_mq_remove_hw_queues_cpuhp(q);
    axlog::warn!(
        "blk_mq_realloc_hw_ctxs: blk_mq_remove_hw_queues_cpuhp is not implemented in this shim"
    );

    // register cpuhp for new initialized hctxs
    // blk_mq_add_hw_queues_cpuhp(q);
    axlog::warn!(
        "blk_mq_realloc_hw_ctxs: blk_mq_add_hw_queues_cpuhp is not implemented in this shim"
    );
}

unsafe fn __blk_mq_realloc_hw_ctxs(set: &mut blk_mq_tag_set, q: &mut request_queue) {
    axlog::warn!("[__blk_mq_realloc_hw_ctxs] is called");
    // for i in 0..set.nr_hw_queues {
    //     let old_node = 0;
    //     let old_hctx: *mut blk_mq_hw_ctx = core::ptr::null_mut();
    // }
    q.nr_hw_queues = set.nr_hw_queues;
}

#[repr(C)]
#[derive(Copy, Clone)]
struct blk_mq_ctxs {
    kobj: kobject,
    // percpu pointer to blk_mq_ctx
    queue_ctx: *mut blk_mq_ctx,
}

#[repr(C)]
#[derive(Copy, Clone)]
#[repr(align(64))]
struct blk_mq_ctx_inner {
    lock: spinlock_t,
    rq_lists: [list_head; hctx_type_HCTX_MAX_TYPES as usize],
}

#[repr(C)]
#[derive(Copy, Clone)]
#[repr(align(64))]
struct blk_mq_ctx {
    inner: blk_mq_ctx_inner,
    cpu: uint,
    index_hw: [u16; hctx_type_HCTX_MAX_TYPES as usize],
    hctxs: [*mut blk_mq_hw_ctx; hctx_type_HCTX_MAX_TYPES as usize],

    queue: *mut request_queue,
    ctxs: *mut blk_mq_ctxs,
    kobj: kobject,
}

// All allocations will be freed in release handler of q->mq_kobj
unsafe fn blk_mq_alloc_ctxs(req_q: &mut request_queue) -> c_int {
    axlog::warn!("[blk_mq_alloc_ctxs] is called");
    let layout = Layout::from_size_align(
        core::mem::size_of::<blk_mq_ctxs>(),
        core::mem::align_of::<blk_mq_ctxs>(),
    )
    .unwrap();
    let ctxs_ptr = alloc::alloc::alloc(layout) as *mut blk_mq_ctxs;
    assert!(
        !ctxs_ptr.is_null(),
        "blk_mq_alloc_ctxs: failed to allocate blk_mq_ctxs"
    );

    let ctxs = ctxs_ptr.as_mut().unwrap();
    let percpu_layout = Layout::from_size_align(
        core::mem::size_of::<blk_mq_ctx>() * nr_cpu_ids as usize,
        core::mem::align_of::<blk_mq_ctx>(),
    )
    .unwrap();
    ctxs.queue_ctx = alloc::alloc::alloc(percpu_layout) as *mut blk_mq_ctx;
    assert!(
        !ctxs.queue_ctx.is_null(),
        "blk_mq_alloc_ctxs: failed to allocate percpu blk_mq_ctx"
    );

    for cpu in 0..nr_cpu_ids {
        let ctx = ctxs.queue_ctx.add(cpu as usize).as_mut().unwrap();
        ctx.ctxs = ctxs_ptr;
    }
    req_q.mq_kobj = &mut ctxs.kobj;
    req_q.queue_ctx = ctxs.queue_ctx as _;
    0
}
