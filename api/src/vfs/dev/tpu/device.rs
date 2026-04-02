//! TPU 设备抽象
//!
//! 提供高层 API 供操作系统调用

use alloc::{collections::VecDeque, sync::Arc};
use core::{
    cell::Cell,
    sync::atomic::{AtomicU32, Ordering},
};

use axsync::Mutex;

// use log::{debug, error, info, warn};
use super::{
    TDMA_PHYS_BASE, TIU_PHYS_BASE, error::TpuError, platform::TpuRuntimeState, tdma::TdmaRegs,
    tiu::TiuRegs, types::*,
};
use crate::{
    file::{get_file_like, ion::IonBufferFile},
    vfs::{
        DeviceOps,
        dev::ion::{IonBufferManager, IonHandle, global_ion_buffer_manager},
    },
};

/// TPU 设备状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpuState {
    /// 未初始化
    Uninitialized,
    /// 空闲
    Idle,
    /// 运行中
    Running,
    /// 已挂起
    Suspended,
}

/// TPU 任务提交路径
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpuSubmitPath {
    /// 普通描述符模式
    DesNormal = 0,
}

/// TPU 任务节点
#[derive(Debug)]
pub struct TpuTaskNode {
    /// 进程 ID
    pub pid: u32,
    /// 序列号
    pub seq_no: u32,
    /// DMA buffer 文件描述符
    pub dmabuf_fd: i32,
    /// DMA buffer 虚拟地址
    pub dmabuf_vaddr: usize,
    /// DMA buffer 物理地址
    pub dmabuf_paddr: u64,
    /// 提交路径
    pub tpu_path: TpuSubmitPath,
    /// 执行结果
    pub ret: i32,
}

/// TPU 内核工作状态
pub struct TpuKernelWork {
    /// 任务队列
    pub task_list: VecDeque<TpuTaskNode>,
    /// 完成队列
    pub done_list: VecDeque<TpuTaskNode>,
}

impl Default for TpuKernelWork {
    fn default() -> Self {
        Self {
            task_list: VecDeque::new(),
            done_list: VecDeque::new(),
        }
    }
}

/// TPU 设备内部状态
struct TpuDeviceInner {
    /// TDMA 寄存器
    tdma: TdmaRegs,
    /// TIU 寄存器
    tiu: TiuRegs,
    /// 设备状态
    state: TpuState,
    /// 运行时状态
    runtime: TpuRuntimeState,
    /// 任务工作队列
    kernel_work: TpuKernelWork,
}

/// TPU 设备
pub struct TpuDevice {
    /// 内部状态 (使用 Mutex 保护)
    inner: Mutex<TpuDeviceInner>,
    /// 序列号计数器
    seq_counter: AtomicU32,
    /// Ion buffer 管理器引用
    ion_manager: Option<Arc<IonBufferManager>>,
}

impl TpuDevice {
    /// 创建未初始化的 TPU 设备
    ///
    /// 使用默认物理地址，需要提供虚拟地址偏移
    ///
    /// # Safety
    /// 调用者必须确保偏移计算后的虚拟地址有效
    pub unsafe fn new() -> Self {
        let virt_offset = 0xffff_ffc0_0000_0000u64 as isize;
        let tdma_vaddr = (TDMA_PHYS_BASE as isize + virt_offset) as *mut u8;
        let tiu_vaddr = (TIU_PHYS_BASE as isize + virt_offset) as *mut u8;

        Self {
            inner: Mutex::new(TpuDeviceInner {
                tdma: unsafe { TdmaRegs::new(tdma_vaddr) },
                tiu: unsafe { TiuRegs::new(tiu_vaddr) },
                state: TpuState::Uninitialized,
                runtime: TpuRuntimeState::default(),
                kernel_work: TpuKernelWork::default(),
            }),
            seq_counter: AtomicU32::new(0),
            ion_manager: Some(global_ion_buffer_manager()),
        }
    }

    /// 使用指定的虚拟地址和 Ion 管理器创建 TPU 设备
    ///
    /// # Safety
    /// 调用者必须确保虚拟地址有效
    pub unsafe fn with_ion_manager(
        tdma_vaddr: *mut u8,
        tiu_vaddr: *mut u8,
        ion_manager: Arc<IonBufferManager>,
    ) -> Self {
        Self {
            inner: Mutex::new(TpuDeviceInner {
                tdma: unsafe { TdmaRegs::new(tdma_vaddr) },
                tiu: unsafe { TiuRegs::new(tiu_vaddr) },
                state: TpuState::Uninitialized,
                runtime: TpuRuntimeState::default(),
                kernel_work: TpuKernelWork::default(),
            }),
            seq_counter: AtomicU32::new(0),
            ion_manager: Some(ion_manager),
        }
    }

    /// 使用指定的虚拟地址创建 TPU 设备
    ///
    /// # Safety
    /// 调用者必须确保虚拟地址有效
    pub unsafe fn from_vaddr(tdma_vaddr: *mut u8, tiu_vaddr: *mut u8) -> Self {
        Self {
            inner: Mutex::new(TpuDeviceInner {
                tdma: unsafe { TdmaRegs::new(tdma_vaddr) },
                tiu: unsafe { TiuRegs::new(tiu_vaddr) },
                state: TpuState::Uninitialized,
                runtime: TpuRuntimeState::default(),
                kernel_work: TpuKernelWork::default(),
            }),
            seq_counter: AtomicU32::new(0),
            ion_manager: Some(global_ion_buffer_manager()),
        }
    }

    /// 设置 Ion buffer 管理器
    pub fn set_ion_manager(&mut self, manager: Arc<IonBufferManager>) {
        self.ion_manager = Some(manager);
    }

    /// 初始化 TPU 设备 (probe)
    pub fn init(&self) -> Result<(), TpuError> {
        let mut inner = self.inner.lock();

        // 重置命令 ID
        super::platform::resync_cmd_id(&inner.tdma, &inner.tiu);

        inner.state = TpuState::Idle;
        inner.runtime = TpuRuntimeState::default();

        info!("TPU device initialized");
        Ok(())
    }

    /// 获取设备状态
    pub fn state(&self) -> TpuState {
        self.inner.lock().state
    }

    /// 检查设备是否就绪
    pub fn is_ready(&self) -> bool {
        self.inner.lock().state == TpuState::Idle
    }

    /// 处理 TDMA 中断
    ///
    /// 应该在你的 OS 中断处理程序中调用此函数
    ///
    /// 返回是否有错误发生
    pub fn handle_irq(&self) -> bool {
        let mut inner = self.inner.lock();
        // 先获取需要的引用，避免同时借用
        let tdma = &inner.tdma as *const TdmaRegs;
        let tiu = &inner.tiu as *const TiuRegs;
        let runtime = &mut inner.runtime;
        unsafe { super::platform::handle_tdma_irq(&*tdma, &*tiu, runtime) }
    }

    /// 获取下一个序列号
    fn next_seq_no(&self) -> u32 {
        self.seq_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// 提交 DMA buffer 任务
    fn submit_dmabuf(&self, arg: usize) -> Result<usize, TpuError> {
        // 从用户空间读取参数
        let submit_arg = unsafe { &*(arg as *const CviSubmitDmaArg) };

        debug!(
            "[TPU] submit dmabuf: fd={}, seq_no={}",
            submit_arg.fd, submit_arg.seq_no
        );

        // 从文件描述符获取 IonBufferFile
        let fd = submit_arg.fd as i32;
        let file = get_file_like(fd).map_err(|_| {
            error!("[TPU] Failed to get file for fd={}", fd);
            TpuError::InvalidDmabuf
        })?;

        // 尝试转换为 IonBufferFile (使用 downcast_arc)
        let ion_file: Arc<IonBufferFile> = file.downcast_arc::<IonBufferFile>().map_err(|_| {
            error!("[TPU] fd={} is not an IonBufferFile", fd);
            TpuError::InvalidDmabuf
        })?;

        // 获取缓冲区信息
        let buffer_info = ion_file.info();
        debug!(
            "[TPU] dmabuf info: handle={}, size={}, paddr=0x{:x}",
            buffer_info.handle, buffer_info.size, buffer_info.phys_addr
        );

        // 从 Ion 管理器获取完整的 buffer 信息
        let ion_manager = self.ion_manager.as_ref().ok_or(TpuError::NotInitialized)?;
        let handle = IonHandle(buffer_info.handle);
        let buffer = ion_manager
            .get_buffer(handle)
            .map_err(|_| TpuError::InvalidDmabuf)?;

        let dmabuf_vaddr = buffer.dma_info.cpu_addr.as_ptr() as usize;
        let dmabuf_paddr = buffer.dma_info.bus_addr.as_u64();

        debug!(
            "[TPU] Buffer: vaddr=0x{:x}, paddr=0x{:x}",
            dmabuf_vaddr, dmabuf_paddr
        );

        // 创建任务节点
        let task = TpuTaskNode {
            pid: 0, // 当前没有进程 ID 概念，可以后续扩展
            seq_no: submit_arg.seq_no,
            dmabuf_fd: submit_arg.fd,
            dmabuf_vaddr,
            dmabuf_paddr,
            tpu_path: TpuSubmitPath::DesNormal,
            ret: 0,
        };

        // 添加到任务队列
        let mut inner = self.inner.lock();
        inner.kernel_work.task_list.push_back(task);

        // 直接执行任务 (简化版本，不使用工作线程)
        self.process_task_locked(&mut inner)?;

        Ok(0)
    }

    /// 处理任务 (内部函数，需要持有锁)
    fn process_task_locked(&self, inner: &mut TpuDeviceInner) -> Result<(), TpuError> {
        while let Some(mut task) = inner.kernel_work.task_list.pop_front() {
            // 初始化 TPU
            super::platform::resync_cmd_id(&inner.tdma, &inner.tiu);
            inner.runtime.irq_received = false;

            // 执行 DMA buffer
            let result =
                self.run_dmabuf_internal(inner, task.dmabuf_vaddr as *const u8, task.dmabuf_paddr);

            task.ret = match result {
                Ok(_) => 0,
                Err(e) => {
                    error!("TPU run dmabuf failed: {:?}", e);
                    -1
                }
            };

            // 移动到完成队列
            inner.kernel_work.done_list.push_back(task);
        }

        Ok(())
    }

    /// 内部执行 DMA buffer
    fn run_dmabuf_internal(
        &self,
        inner: &mut TpuDeviceInner,
        dmabuf_vaddr: *const u8,
        dmabuf_paddr: u64,
    ) -> Result<(), TpuError> {
        if inner.state != TpuState::Idle && inner.state != TpuState::Uninitialized {
            return Err(TpuError::NotInitialized);
        }

        inner.state = TpuState::Running;

        // 简化版超时检查 (使用 Cell 实现内部可变性)
        let timeout_counter = Cell::new(0u64);
        let timeout_limit = 1_000_000_000u64; // 大约 10 秒

        let wait_irq = || -> Result<(), TpuError> {
            // 轮询等待中断
            // 简化实现：直接返回 Ok，由 poll_cmdbuf_done 处理超时
            let mut counter = timeout_counter.get();
            while counter < timeout_limit {
                counter += 1;
                timeout_counter.set(counter);
                core::hint::spin_loop();
                // 简化：假设执行一定迭代后完成
                if counter > 10000 {
                    break;
                }
            }
            if counter >= timeout_limit {
                return Err(TpuError::Timeout);
            }
            Ok(())
        };

        let timeout_checker = || -> bool { timeout_counter.get() > timeout_limit };

        // 使用指针避免同时借用
        let tdma = &inner.tdma as *const TdmaRegs;
        let tiu = &inner.tiu as *const TiuRegs;
        let runtime = &mut inner.runtime;

        let result = unsafe {
            super::platform::run_dmabuf(
                &*tdma,
                &*tiu,
                dmabuf_vaddr,
                dmabuf_paddr,
                runtime,
                wait_irq,
                timeout_checker,
            )
        };

        inner.state = TpuState::Idle;
        result
    }

    /// 等待 DMA buffer 完成
    fn wait_dmabuf(&self, arg: usize) -> Result<usize, TpuError> {
        let wait_arg = unsafe { &mut *(arg as *mut CviWaitDmaArg) };

        debug!("TPU wait dmabuf: seq_no={}", wait_arg.seq_no);

        let mut inner = self.inner.lock();

        // 在完成队列中查找
        let mut found_idx = None;
        for (idx, task) in inner.kernel_work.done_list.iter().enumerate() {
            if task.seq_no == wait_arg.seq_no {
                found_idx = Some(idx);
                break;
            }
        }

        if let Some(idx) = found_idx {
            let task = inner.kernel_work.done_list.remove(idx).unwrap();
            wait_arg.ret = task.ret;
            debug!(
                "TPU wait dmabuf completed: seq_no={}, ret={}",
                wait_arg.seq_no, task.ret
            );
            Ok(0)
        } else {
            warn!("TPU wait dmabuf: seq_no {} not found", wait_arg.seq_no);
            wait_arg.ret = -1;
            Err(TpuError::NotInitialized)
        }
    }

    /// 刷新 DMA buffer 缓存 (通过物理地址)
    fn cache_flush(&self, arg: usize) -> Result<usize, TpuError> {
        let flush_arg = unsafe { &*(arg as *const CviCacheOpArg) };

        debug!(
            "TPU cache flush: paddr=0x{:x}, size={}",
            flush_arg.paddr, flush_arg.size
        );

        // 在 RISC-V 上执行 cache flush
        #[cfg(target_arch = "riscv64")]
        {
            // 使用 fence 指令确保内存一致性
            unsafe {
                core::arch::asm!("fence iorw, iorw");
            }
        }

        Ok(0)
    }

    /// 无效化 DMA buffer 缓存 (通过物理地址)
    fn cache_invalidate(&self, arg: usize) -> Result<usize, TpuError> {
        let invalidate_arg = unsafe { &*(arg as *const CviCacheOpArg) };

        debug!(
            "TPU cache invalidate: paddr=0x{:x}, size={}",
            invalidate_arg.paddr, invalidate_arg.size
        );

        // 在 RISC-V 上执行 cache invalidate
        #[cfg(target_arch = "riscv64")]
        {
            unsafe {
                core::arch::asm!("fence iorw, iorw");
            }
        }

        Ok(0)
    }

    /// 刷新 DMA buffer 缓存 (通过 fd)
    /// todo: implement actual cache operations
    fn dmabuf_flush_fd(&self, arg: usize) -> Result<usize, TpuError> {
        let fd = arg as i32;
        debug!("TPU dmabuf flush fd: {}", fd);

        // 从 Ion 获取 buffer 并刷新
        if let Some(ref ion_manager) = self.ion_manager {
            let handle = IonHandle(fd as u32);
            if let Ok(buffer) = ion_manager.get_buffer(handle) {
                let paddr = buffer.dma_info.bus_addr.as_u64();
                let size = buffer.size;

                #[cfg(target_arch = "riscv64")]
                unsafe {
                    core::arch::asm!("fence iorw, iorw");
                }

                debug!("Flushed buffer: paddr=0x{:x}, size={}", paddr, size);
            }
        }

        Ok(0)
    }

    /// 无效化 DMA buffer 缓存 (通过 fd)
    /// todo: implement actual cache operations
    fn dmabuf_invld_fd(&self, arg: usize) -> Result<usize, TpuError> {
        let fd = arg as i32;
        debug!("TPU dmabuf invalidate fd: {}", fd);

        // 从 Ion 获取 buffer 并无效化
        if let Some(ref ion_manager) = self.ion_manager {
            let handle = IonHandle(fd as u32);
            if let Ok(_buffer) = ion_manager.get_buffer(handle) {
                #[cfg(target_arch = "riscv64")]
                unsafe {
                    core::arch::asm!("fence iorw, iorw");
                }
            }
        }

        Ok(0)
    }

    /// 挂起 TPU
    pub fn suspend(&self) -> Result<(), TpuError> {
        let mut inner = self.inner.lock();

        if inner.state == TpuState::Suspended {
            return Ok(());
        }

        // 使用指针避免同时借用
        let tdma = &inner.tdma as *const TdmaRegs;
        let tiu = &inner.tiu as *const TiuRegs;
        let reg_backup = &mut inner.runtime.reg_backup;
        unsafe {
            super::platform::backup_registers(&*tdma, &*tiu, reg_backup);
        }
        inner.state = TpuState::Suspended;

        info!("TPU suspended");
        Ok(())
    }

    /// 恢复 TPU
    pub fn resume(&self) -> Result<(), TpuError> {
        let mut inner = self.inner.lock();

        if inner.state != TpuState::Suspended {
            return Err(TpuError::NotInitialized);
        }

        // 使用指针避免同时借用
        let tdma = &inner.tdma as *const TdmaRegs;
        let tiu = &inner.tiu as *const TiuRegs;
        let reg_backup = &inner.runtime.reg_backup;
        unsafe {
            super::platform::restore_registers(&*tdma, &*tiu, reg_backup);
        }
        inner.state = TpuState::Idle;

        info!("TPU resumed");
        Ok(())
    }

    /// 重置 TPU
    pub fn reset(&self) {
        let mut inner = self.inner.lock();

        super::platform::resync_cmd_id(&inner.tdma, &inner.tiu);
        inner.runtime = TpuRuntimeState::default();
        inner.state = TpuState::Idle;

        info!("TPU reset");
    }
}

// 实现 Send 和 Sync
unsafe impl Send for TpuDevice {}
unsafe impl Sync for TpuDevice {}

impl DeviceOps for TpuDevice {
    fn read_at(&self, _buf: &mut [u8], _offset: u64) -> axfs_ng_vfs::VfsResult<usize> {
        Ok(0)
    }

    fn write_at(&self, _buf: &[u8], _offset: u64) -> axfs_ng_vfs::VfsResult<usize> {
        Ok(0)
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> axfs_ng_vfs::VfsResult<usize> {
        debug!("TPU ioctl: cmd=0x{:x}, arg=0x{:x}", cmd, arg);

        let result = match cmd {
            CVITPU_SUBMIT_DMABUF => self.submit_dmabuf(arg),
            CVITPU_DMABUF_FLUSH_FD => self.dmabuf_flush_fd(arg),
            CVITPU_DMABUF_INVLD_FD => self.dmabuf_invld_fd(arg),
            CVITPU_DMABUF_FLUSH => self.cache_flush(arg),
            CVITPU_DMABUF_INVLD => self.cache_invalidate(arg),
            CVITPU_WAIT_DMABUF => self.wait_dmabuf(arg),
            CVITPU_PIO_MODE => {
                warn!("TPU PIO mode not implemented");
                Ok(0)
            }
            CVITPU_LOAD_TEE | CVITPU_SUBMIT_TEE | CVITPU_UNLOAD_TEE => {
                warn!("TPU TEE operations not supported");
                Err(TpuError::NotInitialized)
            }
            _ => {
                warn!("Unknown TPU ioctl command: 0x{:x}", cmd);
                Err(TpuError::NotInitialized)
            }
        };

        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                error!("TPU ioctl error: {:?}", e);
                Err(axerrno::AxError::Unsupported)
            }
        }
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}
