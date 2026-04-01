//! Ion 设备实现

use alloc::sync::Arc;
use core::{any::Any, ptr};

use axfs_ng_vfs::{NodeFlags, VfsResult};
use memory_addr::PhysAddrRange;
use starry_core::vfs::{DeviceMmap, DeviceOps};

use super::{
    buffer::IonBufferManager,
    global_ion_buffer_manager,
    heap::IonHeapManager,
    types::{ioctl::*, *},
};
use crate::file::{
    add_file_like,
    ion::{IonBufferFile, IonBufferInfo},
};

/// Ion 设备
pub struct IonDevice {
    /// 堆管理器
    heap_manager: IonHeapManager,
    /// 缓冲区管理器 (使用全局共享)
    buffer_manager: Arc<IonBufferManager>,
}

impl IonDevice {
    /// 创建 Ion 设备
    pub fn new() -> Self {
        Self {
            heap_manager: IonHeapManager::new(),
            buffer_manager: global_ion_buffer_manager(),
        }
    }

    /// 处理 ION_IOC_ALLOC 命令
    fn handle_alloc(&self, user_ptr: usize) -> VfsResult<usize> {
        debug!("Processing ION_IOC_ALLOC");

        // 从用户空间读取分配数据
        let alloc_data = unsafe { ptr::read(user_ptr as *const IonAllocData) };

        debug!(
            "Alloc request: len={}, heap_id_mask=0x{:x}, flags=0x{:x}",
            alloc_data.len, alloc_data.heap_id_mask, alloc_data.flags
        );

        // 选择堆类型（简化处理，优先选择 DMA coherent）
        let heap_type = if (alloc_data.heap_id_mask & (1 << IonHeapType::DmaCoherent as u32)) != 0 {
            IonHeapType::DmaCoherent
        } else if (alloc_data.heap_id_mask & (1 << IonHeapType::Carveout as u32)) != 0 {
            IonHeapType::Carveout
        } else if (alloc_data.heap_id_mask & (1 << IonHeapType::System as u32)) != 0 {
            IonHeapType::System
        } else {
            error!(
                "No supported heap type in mask: 0x{:x}",
                alloc_data.heap_id_mask
            );
            return Err(axerrno::AxError::InvalidInput);
        };

        // 分配缓冲区
        let buffer = self
            .heap_manager
            .alloc_buffer(
                alloc_data.len as usize,
                1,
                heap_type,
                IonFlags(alloc_data.flags),
            )
            .map_err(|e| axerrno::AxError::from(e))?;

        // 注册缓冲区
        self.buffer_manager
            .register_buffer(buffer.clone())
            .map_err(|e| axerrno::AxError::from(e))?;

        // 创建 IonBufferFile 并添加到文件描述符表
        let phys_addr = buffer.dma_info.bus_addr.as_u64() as usize;
        let buffer_info = IonBufferInfo {
            phys_addr,
            size: buffer.size,
            handle: buffer.handle.as_u32(),
        };
        let ion_file = IonBufferFile::new(buffer_info);
        let fd = add_file_like(alloc::sync::Arc::new(ion_file), false)
            .map_err(|_| axerrno::AxError::TooManyOpenFiles)?;

        // 返回结果
        let mut result_data = alloc_data;
        result_data.fd = fd as u32;
        result_data.paddr = phys_addr as u64;

        unsafe {
            ptr::write(user_ptr as *mut IonAllocData, result_data);
        }

        info!(
            "Allocated Ion buffer: fd={}, handle={}, phys_addr=0x{:x}, size={}",
            fd,
            buffer.handle.as_u32(),
            phys_addr,
            alloc_data.len
        );

        Ok(0)
    }

    /// 处理 ION_IOC_FREE 命令
    fn handle_free(&self, user_ptr: usize) -> VfsResult<usize> {
        debug!("Processing ION_IOC_FREE");

        // 从用户空间读取句柄数据
        let handle_data = unsafe { ptr::read(user_ptr as *const IonHandleData) };

        let handle = IonHandle(handle_data.handle);
        debug!("Freeing buffer with handle: {:?}", handle);

        // 取消注册缓冲区
        let buffer = self
            .buffer_manager
            .unregister_buffer(handle)
            .map_err(|e| axerrno::AxError::from(e))?;

        // 释放缓冲区
        self.heap_manager
            .free_buffer(buffer)
            .map_err(|e| axerrno::AxError::from(e))?;

        info!("Freed Ion buffer: handle={}", handle_data.handle);
        Ok(0)
    }

    /// 处理 ION_IOC_IMPORT 命令
    fn handle_import(&self, user_ptr: usize) -> VfsResult<usize> {
        debug!("Processing ION_IOC_IMPORT");

        // 从用户空间读取 FD 数据
        let fd_data = unsafe { ptr::read(user_ptr as *const IonFdData) };

        debug!("Import request: fd={}", fd_data.fd);

        // 简化处理：将 fd 作为 handle 直接使用
        // 实际实现中应该检查 fd 的有效性
        let handle = IonHandle(fd_data.fd as u32);

        // 返回结果
        let mut result_data = fd_data;
        result_data.handle = handle.as_u32();

        unsafe {
            ptr::write(user_ptr as *mut IonFdData, result_data);
        }

        info!(
            "Imported Ion buffer: fd={}, handle={}",
            fd_data.fd, result_data.handle
        );
        Ok(0)
    }

    /// 处理 ION_IOC_HEAP_QUERY 命令
    fn handle_heap_query(&self, user_ptr: usize) -> VfsResult<usize> {
        debug!("Processing ION_IOC_HEAP_QUERY");

        // 从用户空间读取查询数据
        let mut heap_query = unsafe { ptr::read(user_ptr as *const IonHeapQuery) };

        debug!(
            "Heap query request: cnt={}, heaps=0x{:x}",
            heap_query.cnt, heap_query.heaps
        );

        // 定义支持的堆类型信息
        let supported_heaps = [
            (IonHeapType::System, "system", 0),
            (IonHeapType::DmaCoherent, "dma_coherent", 1),
            (IonHeapType::Carveout, "carveout", 2),
        ];

        let available_heap_count = supported_heaps.len() as u32;
        let requested_count = heap_query.cnt.min(available_heap_count);

        // 如果用户提供了堆缓冲区指针，填充堆信息
        if heap_query.heaps != 0 && requested_count > 0 {
            let heap_data_ptr = heap_query.heaps as *mut IonHeapData;

            for (i, &(heap_type, name, heap_id)) in supported_heaps
                .iter()
                .enumerate()
                .take(requested_count as usize)
            {
                let mut heap_data = IonHeapData {
                    name: [0; MAX_HEAP_NAME],
                    type_: heap_type as u32,
                    heap_id,
                    reserved0: 0,
                    reserved1: 0,
                    reserved2: 0,
                };

                // 复制堆名称
                let name_bytes = name.as_bytes();
                let copy_len = name_bytes.len().min(MAX_HEAP_NAME - 1);
                heap_data.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);

                // 写入堆数据
                unsafe {
                    ptr::write(heap_data_ptr.add(i), heap_data);
                }

                info!(
                    "Added heap {}: type={}, heap_id={}, name={}",
                    i, heap_type as u32, heap_id, name
                );
            }
        }

        // 更新返回的堆数量
        heap_query.cnt = available_heap_count;

        // 写回结果
        unsafe {
            ptr::write(user_ptr as *mut IonHeapQuery, heap_query);
        }

        info!(
            "Heap query completed: {} heaps available, {} requested",
            available_heap_count, requested_count
        );
        Ok(0)
    }
}

impl DeviceOps for IonDevice {
    fn read_at(&self, _buf: &mut [u8], _offset: u64) -> VfsResult<usize> {
        // Ion 设备不支持直接读写
        Ok(0)
    }

    fn write_at(&self, _buf: &[u8], _offset: u64) -> VfsResult<usize> {
        // Ion 设备不支持直接读写
        Ok(0)
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> VfsResult<usize> {
        match cmd {
            ION_IOC_HEAP_QUERY => self.handle_heap_query(arg),
            ION_IOC_ALLOC => self.handle_alloc(arg),
            ION_IOC_FREE => self.handle_free(arg),
            ION_IOC_IMPORT => self.handle_import(arg),
            _ => {
                warn!("Unsupported Ion ioctl command: 0x{:x}", cmd);
                Err(axerrno::AxError::Unsupported)
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn flags(&self) -> NodeFlags {
        NodeFlags::NON_CACHEABLE
    }

    fn mmap(&self, offset: usize, length: usize) -> DeviceMmap {
        // offset 被用作 Ion buffer 的 handle
        // 用户空间通过 mmap(fd, offset=handle, ...) 来映射特定的 Ion buffer
        let handle = IonHandle(offset as u32);

        match self.buffer_manager.get_buffer(handle) {
            Ok(buffer) => {
                // 获取缓冲区的物理地址
                let phys_addr = buffer.dma_info.bus_addr.as_u64() as usize;
                let size = if length > 0 {
                    length.min(buffer.size)
                } else {
                    buffer.size
                };

                debug!(
                    "Ion mmap: handle={}, phys_addr=0x{:x}, size={}",
                    offset, phys_addr, size
                );

                // 标记缓冲区为已映射
                buffer.set_mapped();

                DeviceMmap::Physical(PhysAddrRange::from_start_size(
                    memory_addr::PhysAddr::from(phys_addr),
                    size,
                ))
            }
            Err(e) => {
                warn!(
                    "Ion mmap failed: cannot find buffer with handle {}: {:?}",
                    offset, e
                );
                DeviceMmap::None
            }
        }
    }
}

impl Drop for IonDevice {
    fn drop(&mut self) {
        warn!("Ion device is being dropped, cleaning up buffers");
        self.buffer_manager.cleanup_all();
    }
}
