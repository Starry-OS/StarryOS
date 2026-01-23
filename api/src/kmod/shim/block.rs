use core::{
    ffi::{CStr, c_char, c_int},
    sync::atomic::AtomicU16,
};

use kmod::{capi_fn, kbindings::*};
use spin::Once;

static MAJOR_NUMBER: AtomicU16 = AtomicU16::new(1);

/// __register_blkdev - register a new block device
///
/// @major: the requested major device number [1..BLKDEV_MAJOR_MAX-1]. If
///         @major = 0, try to allocate any unused major number.
/// @name: the name of the new block device as a zero terminated string
/// @probe: pre-devtmpfs / pre-udev callback used to create disks when their
/// 	   pre-created device node is accessed. When a probe call uses
/// 	   add_disk() and it fails the driver must cleanup resources. This
/// 	   interface may soon be removed.
///
/// The @name must be unique within the system.
///
/// The return value depends on the @major input parameter:
///
///  - if a major device number was requested in range [1..BLKDEV_MAJOR_MAX-1]
///    then the function returns zero on success, or a negative error code
///  - if any unused major number was requested with @major = 0 parameter then
///    the return value is the allocated major number in range
///    [1..BLKDEV_MAJOR_MAX-1] or a negative error code otherwise
///
/// See Documentation/admin-guide/devices.txt for the list of allocated
/// major numbers.
///
/// Use register_blkdev instead for any new code.
#[capi_fn]
unsafe extern "C" fn __register_blkdev(
    major: u32,
    name: *const c_char,
    probe: Option<extern "C" fn(dev_t: dev_t)>,
) -> c_int {
    let dev_name = CStr::from_ptr(name);
    axlog::warn!(
        "__register_blkdev called with major: {}, name: {:?}",
        major,
        dev_name
    );
    assert!(probe.is_none(), "probe function is not supported");
    if (1..BLKDEV_MAJOR_MAX as u32).contains(&major) {
        // Specific major requested
        axlog::warn!("Registered block device with major number {}", major);
        0
    } else if major == 0 {
        // Allocate any major
        let allocated_major = MAJOR_NUMBER.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        if (allocated_major as u32) < BLKDEV_MAJOR_MAX {
            axlog::warn!("Allocated block device major number {}", allocated_major);
            allocated_major as c_int
        } else {
            panic!("No more major numbers available");
        }
    } else {
        panic!("Invalid major number requested: {}", major);
    }
}

/// device_add_disk - add disk information to kernel list
///
/// # Arguments
/// - parent: parent device for the disk
/// - disk: per-device partitioning information
/// - groups: Additional per-device sysfs groups
///
/// This function registers the partitioning information in `disk`
/// with the kernel.
#[must_use]
#[capi_fn]
unsafe extern "C" fn device_add_disk(
    parent: *mut device,
    disk: *mut gendisk,
    groups: *mut *const attribute_group,
) -> c_int {
    add_disk_fwnode(parent, disk, groups, core::ptr::null_mut())
}

/// add_disk_fwnode - add disk information to kernel list with fwnode
/// # Arguments
/// - parent: parent device for the disk
/// - disk: per-device partitioning information
/// - groups: Additional per-device sysfs groups
/// - fwnode: attached disk fwnode
///
/// This function registers the partitioning information in @disk
/// with the kernel. Also attach a fwnode to the disk device.
#[must_use]
#[capi_fn]
unsafe extern "C" fn add_disk_fwnode(
    _parent: *mut device,
    disk: *mut gendisk,
    _groups: *mut *const attribute_group,
    _fwnode: *mut fwnode_handle,
) -> c_int {
    axlog::warn!("add_disk_fwnode called");

    let disk = disk.as_mut().unwrap();
    let queue = disk.queue.as_mut().unwrap();
    assert!(
        !queue.mq_ops.is_null(),
        "add_disk_fwnode: mq_ops must not be null"
    );
    __add_disk(_parent, disk, _groups, _fwnode);
    0
}
#[capi_fn]
unsafe extern "C" fn __add_disk(
    _parent: *mut device,
    disk: *mut gendisk,
    _groups: *mut *const attribute_group,
    _fwnode: *mut fwnode_handle,
) -> c_int {
    axlog::warn!("__add_disk called");
    let disk = disk.as_mut().unwrap();
    let queue = disk.queue.as_mut().unwrap();

    let disk_ops = disk.fops.as_ref().unwrap();
    // ->submit_bio and ->poll_bio are bypassed for blk-mq drivers.
    assert!(
        disk_ops.submit_bio.is_none() && disk_ops.poll_bio.is_none(),
        "__add_disk: submit_bio and poll_bio must be None, use make_request_fn or mq_ops instead"
    );

    axlog::warn!(
        "Added disk: major: {}, first_minor: {}, hw_queues: {}",
        disk.major,
        disk.first_minor,
        queue.nr_hw_queues
    );

    GENDISK.call_once(|| GenDiskRef(disk));

    0
}

struct GenDiskRef(&'static mut gendisk);

unsafe impl Send for GenDiskRef {}
unsafe impl Sync for GenDiskRef {}

static GENDISK: Once<GenDiskRef> = Once::new();
