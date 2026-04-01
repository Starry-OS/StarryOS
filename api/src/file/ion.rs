//! Ion Buffer 文件类型
//!
//! 实现 FileLike trait，用于支持对 Ion 分配的缓冲区进行 mmap。

use alloc::borrow::Cow;

use axerrno::AxResult;
use axpoll::{IoEvents, Pollable};
use memory_addr::PhysAddrRange;
use starry_core::vfs::DeviceOps;

use super::{FileLike, Kstat};
use crate::vfs::dev::{
    ION_DEVICE,
    ion::types::ioctl::{ION_IOC_FREE, IonHandleData},
};

/// Ion Buffer 的物理地址信息
#[derive(Debug, Clone)]
pub struct IonBufferInfo {
    /// 物理地址
    pub phys_addr: usize,
    /// 缓冲区大小
    pub size: usize,
    /// 缓冲区 handle
    pub handle: u32,
}

/// Ion Buffer 文件
///
/// 用于支持对 Ion 分配的缓冲区进行 mmap
pub struct IonBufferFile {
    /// 缓冲区信息
    info: IonBufferInfo,
}

impl IonBufferFile {
    /// 创建新的 Ion Buffer 文件
    pub fn new(info: IonBufferInfo) -> Self {
        Self { info }
    }

    /// 获取物理地址范围
    pub fn phys_range(&self) -> PhysAddrRange {
        PhysAddrRange::from_start_size(
            memory_addr::PhysAddr::from(self.info.phys_addr),
            self.info.size,
        )
    }

    /// 获取缓冲区信息
    pub fn info(&self) -> &IonBufferInfo {
        &self.info
    }
}

impl Pollable for IonBufferFile {
    fn poll(&self) -> IoEvents {
        IoEvents::IN | IoEvents::OUT
    }

    fn register(&self, _context: &mut core::task::Context<'_>, _events: IoEvents) {
        // Ion buffer 总是就绪
    }
}

impl FileLike for IonBufferFile {
    fn read(&self, _dst: &mut super::IoDst) -> AxResult<usize> {
        // Ion buffer 不支持直接读取
        Err(axerrno::AxError::InvalidInput)
    }

    fn write(&self, _src: &mut super::IoSrc) -> AxResult<usize> {
        // Ion buffer 不支持直接写入
        Err(axerrno::AxError::InvalidInput)
    }

    fn stat(&self) -> AxResult<Kstat> {
        Ok(Kstat {
            size: self.info.size as u64,
            ..Default::default()
        })
    }

    fn path(&self) -> Cow<'_, str> {
        Cow::Borrowed("/dev/ion_buffer")
    }
}

impl Drop for IonBufferFile {
    fn drop(&mut self) {
        debug!(
            "Dropping IonBufferFile, freeing handle: {}",
            self.info.handle
        );
        if let Some(dev) = ION_DEVICE.get() {
            let handle_data = IonHandleData {
                handle: self.info.handle,
            };
            // 调用 ioctl 释放内存
            // 这里忽略了返回值，因为在 drop 中很难处理错误
            let _ = dev.ioctl(ION_IOC_FREE, &handle_data as *const _ as usize);
        } else {
            error!(
                "Failed to find ion device to free buffer handle: {}",
                self.info.handle
            );
        }
    }
}
