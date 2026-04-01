//! Ion (Android ION) memory allocator driver
//!
//! Ion 是一个用于 Android 系统的内存分配器，用于在不同的硬件组件
//! （如 GPU、摄像头、显示器等）之间共享内存缓冲区。
//!
//! 这个实现基于 ArceOS 的 axdma 模块，提供 DMA coherent 内存分配。
//!
//! ## 特性
//!
//! - 支持 DMA coherent 内存分配
//! - 通过 IOCTL 接口进行内存管理
//! - 缓冲区引用计数管理
//! - 支持多种堆类型
//!
//! ## IOCTL 命令
//!
//! - `ION_IOC_ALLOC`: 分配内存缓冲区
//! - `ION_IOC_FREE`: 释放内存缓冲区
//! - `ION_IOC_IMPORT`: 导入外部文件描述符
//!
//! ## 使用示例
//!
//! ```c
//! // 分配 4KB DMA 内存
//! struct ion_allocation_data alloc_data = {
//!     .len = 4096,
//!     .align = 0,
//!     .heap_id_mask = 1 << ION_HEAP_TYPE_DMA,
//!     .flags = 0,
//! };
//! ioctl(ion_fd, ION_IOC_ALLOC, &alloc_data);
//!
//! // 释放内存
//! struct ion_handle_data handle_data = {
//!     .handle = alloc_data.handle,
//! };
//! ioctl(ion_fd, ION_IOC_FREE, &handle_data);
//! ```

mod buffer;
mod device;
mod error;
mod heap;
pub mod types;

use alloc::sync::Arc;

pub use buffer::IonBufferManager;
pub use device::IonDevice;
use spin::Once;
pub use types::IonHandle;

/// 全局共享的 Ion Buffer 管理器
static GLOBAL_ION_BUFFER_MANAGER: Once<Arc<IonBufferManager>> = Once::new();

/// 获取全局 Ion Buffer 管理器
pub fn global_ion_buffer_manager() -> Arc<IonBufferManager> {
    GLOBAL_ION_BUFFER_MANAGER
        .call_once(|| Arc::new(IonBufferManager::new()))
        .clone()
}
