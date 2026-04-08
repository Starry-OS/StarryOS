use alloc::{collections::VecDeque, sync::Arc};
use core::{
    any::Any,
    ptr::NonNull,
};

use axerrno::AxError;
use axfs_ng_vfs::{NodeFlags, VfsResult};
use axsync::Mutex;
use tpu_sg2002::{
    KernelFns, LogLevel, TIMEOUT_US, TimeStamp, TpuConfig, TpuDevice, parse_dmabuf_view,
};

use crate::file::{get_file_like, ion::IonBufferFile};
use crate::vfs::dev::ion::{IonHandle, global_ion_buffer_manager};
use crate::vfs::DeviceOps;

const IOCTL_TPU_BASE: u8 = b'p';

const fn iow(ty: u8, nr: u8, size: usize) -> u32 {
    (1u32 << 30) | ((size as u32) << 16) | ((ty as u32) << 8) | (nr as u32)
}

const fn iowr(ty: u8, nr: u8, size: usize) -> u32 {
    (3u32 << 30) | ((size as u32) << 16) | ((ty as u32) << 8) | (nr as u32)
}

const CVITPU_SUBMIT_DMABUF: u32 = iow(IOCTL_TPU_BASE, 0x01, 8);
const CVITPU_DMABUF_FLUSH_FD: u32 = iow(IOCTL_TPU_BASE, 0x02, 8);
const CVITPU_DMABUF_INVLD_FD: u32 = iow(IOCTL_TPU_BASE, 0x03, 8);
const CVITPU_DMABUF_FLUSH: u32 = iow(IOCTL_TPU_BASE, 0x04, 8);
const CVITPU_DMABUF_INVLD: u32 = iow(IOCTL_TPU_BASE, 0x05, 8);
const CVITPU_WAIT_DMABUF: u32 = iowr(IOCTL_TPU_BASE, 0x06, 8);
const CVITPU_PIO_MODE: u32 = iow(IOCTL_TPU_BASE, 0x07, 8);
const CVITPU_LOAD_TEE: u32 = iowr(IOCTL_TPU_BASE, 0x08, 8);
const CVITPU_SUBMIT_TEE: u32 = iow(IOCTL_TPU_BASE, 0x09, 8);
const CVITPU_UNLOAD_TEE: u32 = iow(IOCTL_TPU_BASE, 0x0A, 8);

const TDMA_PHYS_BASE: usize = 0x0C10_0000;
const TIU_PHYS_BASE: usize = 0x0C10_1000;
const PHY_TO_VIRT_OFFSET: isize = 0xffff_ffc0_0000_0000u64 as isize;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct CviSubmitDmaArg {
    fd: i32,
    seq_no: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct CviWaitDmaArg {
    seq_no: u32,
    ret: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct CviCacheOpArg {
    paddr: u64,
    size: u64,
    dma_fd: i32,
    _padding: i32,
}

#[derive(Debug, Clone, Copy)]
struct DoneTask {
    seq_no: u32,
    ret: i32,
}

struct CviTpuInner {
    dev: TpuDevice<StarryKernelFns>,
    done_list: VecDeque<DoneTask>,
}

unsafe impl Send for CviTpuInner {}

pub struct CviTpuDevice {
    inner: Mutex<CviTpuInner>,
}

#[derive(Clone, Copy)]
struct StarryKernelFns;

impl KernelFns for StarryKernelFns {
    fn sleep_ms(&self, ms: u32) {
        let start = self.now_us();
        let delay = (ms as u64).saturating_mul(1_000);
        while self.now_us().saturating_sub(start) < delay {
            core::hint::spin_loop();
        }
    }

    fn now_us(&self) -> TimeStamp {
        axhal::time::monotonic_time_nanos() as u64 / 1_000
    }

    fn log(&self, level: LogLevel, msg: &str) {
        match level {
            LogLevel::Error => error!("[cvi-tpu] {msg}"),
            LogLevel::Warn => warn!("[cvi-tpu] {msg}"),
            LogLevel::Info => info!("[cvi-tpu] {msg}"),
            LogLevel::Debug => debug!("[cvi-tpu] {msg}"),
        }
    }

    fn dma_sync_for_device(&self, _paddr: u64, _size: usize) {
        #[cfg(target_arch = "riscv64")]
        unsafe {
            core::arch::asm!("fence iorw, iorw");
        }
    }

    fn dma_sync_for_cpu(&self, _paddr: u64, _size: usize) {
        #[cfg(target_arch = "riscv64")]
        unsafe {
            core::arch::asm!("fence iorw, iorw");
        }
    }
}

impl CviTpuDevice {
    fn now_us() -> u64 {
        axhal::time::monotonic_time_nanos() as u64 / 1_000
    }

    fn ion_info_by_fd(fd: i32) -> VfsResult<crate::file::ion::IonBufferInfo> {
        let file = get_file_like(fd).map_err(|_| AxError::InvalidInput)?;
        let ion_file: Arc<IonBufferFile> = file
            .downcast_arc::<IonBufferFile>()
            .map_err(|_| AxError::InvalidInput)?;
        Ok(ion_file.info().clone())
    }

    fn pop_done_task(&self, seq_no: u32) -> Option<DoneTask> {
        let mut inner = self.inner.lock();
        if let Some(idx) = inner.done_list.iter().position(|task| task.seq_no == seq_no) {
            return inner.done_list.remove(idx);
        }
        None
    }

    pub fn new() -> Self {
        let tdma_vaddr = (TDMA_PHYS_BASE as isize + PHY_TO_VIRT_OFFSET) as *mut u8;
        let tiu_vaddr = (TIU_PHYS_BASE as isize + PHY_TO_VIRT_OFFSET) as *mut u8;
        let tdma_base = NonNull::new(tdma_vaddr).expect("invalid TDMA base");
        let tiu_base = NonNull::new(tiu_vaddr).expect("invalid TIU base");
        let mut dev = TpuDevice::new(tdma_base, tiu_base, TpuConfig::default(), StarryKernelFns);
        let _ = dev.initialize();
        dev.probe_setting();

        Self {
            inner: Mutex::new(CviTpuInner {
                dev,
                done_list: VecDeque::new(),
            }),
        }
    }

    pub fn init(&self) {
        if let Err(err) = self.inner.lock().dev.platform_init() {
            warn!("CVI TPU platform_init failed: {:?}", err);
        }
        info!("CVI TPU device initialized (external crate mode)");
    }

    fn submit_dmabuf(&self, arg: usize) -> VfsResult<usize> {
        let submit_arg = unsafe { &*(arg as *const CviSubmitDmaArg) };

        let ion_info = Self::ion_info_by_fd(submit_arg.fd)?;
        let dmabuf_paddr = ion_info.phys_addr as u64;

        // Resolve the CPU-accessible pointer from Ion manager, instead of
        // guessing VA from PA. This avoids parsing invalid command headers.
        let ion_mgr = global_ion_buffer_manager();
        let ion_buf = ion_mgr
            .get_buffer(IonHandle(ion_info.handle))
            .map_err(|_| AxError::InvalidInput)?;
        let dmabuf_vaddr = ion_buf.dma_info.cpu_addr.as_ptr() as *const u8;

        let parsed = unsafe { parse_dmabuf_view(dmabuf_vaddr, ion_info.size) }
            .map_err(|_| AxError::InvalidInput)?;
        let header = *parsed.header;
        let descs = parsed.descs;

        debug!(
            "CVI TPU submit: fd={}, paddr=0x{:x}, magic_m=0x{:x}, magic_s=0x{:x}, dmabuf_size={}, cpu_desc_count={}, bd_desc_count={}, tdma_desc_count={}",
            submit_arg.fd,
            dmabuf_paddr,
            header.dmabuf_magic_m,
            header.dmabuf_magic_s,
            header.dmabuf_size,
            header.cpu_desc_count,
            header.bd_desc_count,
            header.tdma_desc_count
        );

        // Follow the Linux C driver behavior: preserve userspace seq_no as-is.
        let seq_no = submit_arg.seq_no;

        let mut inner = self.inner.lock();
        let ret = match inner.dev.run_dmabuf(dmabuf_paddr, &header, descs) {
            Ok(_) => 0,
            Err(err) => {
                warn!("CVI TPU run_dmabuf failed: {:?}", err);
                -1
            }
        };
        debug!("CVI TPU submit done: seq_no={}, ret={}", seq_no, ret);
        inner.done_list.push_back(DoneTask { seq_no, ret });
        Ok(0)
    }

    fn wait_dmabuf(&self, arg: usize) -> VfsResult<usize> {
        let wait_arg = unsafe { &mut *(arg as *mut CviWaitDmaArg) };
        let start = Self::now_us();
        let kfns = StarryKernelFns;
        debug!("CVI TPU wait start: seq_no={}", wait_arg.seq_no);

        loop {
            if let Some(task) = self.pop_done_task(wait_arg.seq_no) {
                wait_arg.ret = task.ret;
                debug!("CVI TPU wait done: seq_no={}, ret={}", wait_arg.seq_no, wait_arg.ret);
                return Ok(0);
            }
            if Self::now_us().saturating_sub(start) > TIMEOUT_US {
                wait_arg.ret = -1;
                warn!("CVI TPU wait timeout: seq_no={}", wait_arg.seq_no);
                return Ok(0);
            }
            kfns.sleep_ms(1);
        }
    }

    fn cache_sync_paddr(&self, arg: usize, to_device: bool) -> VfsResult<usize> {
        let cache_arg = unsafe { &*(arg as *const CviCacheOpArg) };
        if cache_arg.size == 0 {
            return Ok(0);
        }
        let kfns = StarryKernelFns;
        if to_device {
            kfns.dma_sync_for_device(cache_arg.paddr, cache_arg.size as usize);
        } else {
            kfns.dma_sync_for_cpu(cache_arg.paddr, cache_arg.size as usize);
        }
        Ok(0)
    }

    fn cache_sync_fd(&self, arg: usize, to_device: bool) -> VfsResult<usize> {
        let fd = arg as i32;
        let ion_info = Self::ion_info_by_fd(fd)?;
        let kfns = StarryKernelFns;
        if to_device {
            kfns.dma_sync_for_device(ion_info.phys_addr as u64, ion_info.size);
        } else {
            kfns.dma_sync_for_cpu(ion_info.phys_addr as u64, ion_info.size);
        }
        Ok(0)
    }

    fn cache_barrier(&self) {
        #[cfg(target_arch = "riscv64")]
        unsafe {
            core::arch::asm!("fence iorw, iorw");
        }
    }
}

impl DeviceOps for CviTpuDevice {
    fn read_at(&self, _buf: &mut [u8], _offset: u64) -> VfsResult<usize> {
        Ok(0)
    }

    fn write_at(&self, _buf: &[u8], _offset: u64) -> VfsResult<usize> {
        Ok(0)
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> VfsResult<usize> {
        match cmd {
            CVITPU_SUBMIT_DMABUF => self.submit_dmabuf(arg),
            CVITPU_WAIT_DMABUF => self.wait_dmabuf(arg),
            CVITPU_DMABUF_FLUSH => self.cache_sync_paddr(arg, true),
            CVITPU_DMABUF_INVLD => self.cache_sync_paddr(arg, false),
            CVITPU_DMABUF_FLUSH_FD => self.cache_sync_fd(arg, true),
            CVITPU_DMABUF_INVLD_FD => self.cache_sync_fd(arg, false),
            CVITPU_PIO_MODE => {
                self.cache_barrier();
                Ok(0)
            }
            CVITPU_LOAD_TEE | CVITPU_SUBMIT_TEE | CVITPU_UNLOAD_TEE => Err(AxError::Unsupported),
            _ => Err(AxError::Unsupported),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn flags(&self) -> NodeFlags {
        NodeFlags::NON_CACHEABLE | NodeFlags::STREAM
    }
}
