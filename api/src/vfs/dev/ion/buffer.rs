//! Ion 缓冲区管理

use alloc::{collections::BTreeMap, sync::Arc};

use axsync::Mutex;

use super::{
    error::{IonError, IonResult},
    types::{IonBuffer, IonHandle},
};

/// Ion 缓冲区管理器
pub struct IonBufferManager {
    /// 已分配的缓冲区映射
    buffers: Mutex<BTreeMap<IonHandle, Arc<IonBuffer>>>,
}

impl IonBufferManager {
    /// 创建新的缓冲区管理器
    pub const fn new() -> Self {
        Self {
            buffers: Mutex::new(BTreeMap::new()),
        }
    }

    /// 注册缓冲区
    pub fn register_buffer(&self, buffer: Arc<IonBuffer>) -> IonResult<()> {
        let mut buffers = self.buffers.lock();
        let handle = buffer.handle;

        if buffers.contains_key(&handle) {
            return Err(IonError::BufferExists);
        }

        buffers.insert(handle, buffer);
        debug!("Registered Ion buffer with handle: {:?}", handle);
        Ok(())
    }

    /// 取消注册缓冲区
    pub fn unregister_buffer(&self, handle: IonHandle) -> IonResult<Arc<IonBuffer>> {
        let mut buffers = self.buffers.lock();
        let buffer = buffers.remove(&handle).ok_or(IonError::BufferNotFound)?;

        debug!("Unregistered Ion buffer with handle: {:?}", handle);
        Ok(buffer)
    }

    /// 获取缓冲区
    pub fn get_buffer(&self, handle: IonHandle) -> IonResult<Arc<IonBuffer>> {
        let buffers = self.buffers.lock();
        buffers
            .get(&handle)
            .cloned()
            .ok_or(IonError::BufferNotFound)
    }

    /// 增加缓冲区引用计数
    pub fn inc_buffer_ref(&self, handle: IonHandle) -> IonResult<usize> {
        let buffers = self.buffers.lock();
        let buffer = buffers.get(&handle).ok_or(IonError::BufferNotFound)?;

        let ref_count = buffer.inc_ref();
        debug!(
            "Increased ref count for handle {:?} to {}",
            handle, ref_count
        );
        Ok(ref_count)
    }

    /// 减少缓冲区引用计数
    pub fn dec_buffer_ref(&self, handle: IonHandle) -> IonResult<usize> {
        let buffers = self.buffers.lock();
        let buffer = buffers.get(&handle).ok_or(IonError::BufferNotFound)?;

        let ref_count = buffer.dec_ref();
        debug!(
            "Decreased ref count for handle {:?} to {}",
            handle, ref_count
        );

        if ref_count == 0 {
            warn!(
                "Buffer ref count reached 0, but buffer still registered: {:?}",
                handle
            );
        }

        Ok(ref_count)
    }

    /// 获取缓冲区数量
    pub fn buffer_count(&self) -> usize {
        self.buffers.lock().len()
    }

    /// 清理所有缓冲区
    pub fn cleanup_all(&self) {
        let mut buffers = self.buffers.lock();
        let count = buffers.len();
        buffers.clear();
        if count > 0 {
            warn!("Cleaned up {} Ion buffers", count);
        }
    }

    /// 获取所有句柄列表（用于调试）
    #[cfg(debug_assertions)]
    pub fn debug_list_handles(&self) -> alloc::vec::Vec<IonHandle> {
        self.buffers.lock().keys().copied().collect()
    }
}
