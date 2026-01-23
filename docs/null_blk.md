# Linux null_blk 驱动分析

本文档分析 Linux 内核中的 null_blk 驱动程序的实现与工作原理。null_blk 驱动是一种虚拟块设备驱动，常用于测试和性能评估场景。

## Linux 接口依赖
```
U __alloc_pages_noprof
U __blk_mq_alloc_disk
U __free_pages
U __kmalloc_cache_node_noprof
U __kmalloc_cache_noprof
U __kmalloc_noprof
U __mmiowb_state
U __mutex_init
U __per_cpu_offset
U __register_blkdev
U __stack_chk_fail
U __stack_chk_guard
U __warn_printk
U _printk
U _raw_spin_lock
U _raw_spin_lock_irq
U badblocks_check
U badblocks_clear
U badblocks_exit
U badblocks_init
U badblocks_set
U badblocks_show
U blk_mq_alloc_tag_set
U blk_mq_complete_request
U blk_mq_end_request
U blk_mq_end_request_batch
U blk_mq_free_tag_set
U blk_mq_map_queues
U blk_mq_start_request
U blk_mq_start_stopped_hw_queues
U blk_mq_stop_hw_queues
U blk_mq_update_nr_hw_queues
U config_group_init
U config_group_init_type_name
U config_item_put
U configfs_register_subsystem
U configfs_unregister_subsystem
U del_gendisk
U device_add_disk
U errno_to_blk_status
U hrtimer_active
U hrtimer_cancel
U hrtimer_forward
U hrtimer_setup
U hrtimer_start_range_ns
U hugetlb_optimize_vmemmap_key
U ida_alloc_range
U ida_free
U kernel_map
U kfree
U kmalloc_caches
U kstrndup
U kstrtobool
U kstrtoint
U kstrtouint
U kstrtoull
U memcpy
U memset
U mutex_lock
U mutex_unlock
U nr_cpu_ids
U param_get_int
U param_ops_bool
U param_ops_int
U param_ops_uint
U param_ops_ulong
U pgtable_l4_enabled
U pgtable_l5_enabled
U put_disk
U qspinlock_key
U radix_tree_delete_item
U radix_tree_gang_lookup
U radix_tree_insert
U radix_tree_lookup
U radix_tree_preload
U set_capacity
U sized_strscpy
U snprintf
U sprintf
U strchr
U strcmp
U strim
U unregister_blkdev
U vmemmap_start_pfn
```
这些依赖可以划分为几个部分：

```
U param_get_int
U param_ops_bool
U param_ops_int
U param_ops_uint
U param_ops_ulong
```
这部分与内核参数处理相关，在kapi中有对应的实现和符号。


```
U strchr
U strcmp
U strim
```
这些是字符串处理函数，kapi中也有对应的实现。

```
U kstrndup
U kstrtobool
U kstrtoint
U kstrtouint
U kstrtoull
```
这些是字符串转换函数，kapi中也有对应的实现。


```
U radix_tree_delete_item
U radix_tree_gang_lookup
U radix_tree_insert
U radix_tree_lookup
U radix_tree_preload
```
这些是基数树相关的函数，kapi中也有对应的实现。