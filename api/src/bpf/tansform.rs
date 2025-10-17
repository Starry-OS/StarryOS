use alloc::{boxed::Box, string::String, sync::Arc, vec, vec::Vec};
use core::{
    ffi::c_void,
    fmt::{Debug, Formatter},
};

use axconfig::plat::CPU_NUM;
use axerrno::{AxError, AxResult};
use axhal::{percpu::this_cpu_id, time::monotonic_time_nanos};
use axio::{Read, Write};
use kbpf_basic::{
    BpfError, KernelAuxiliaryOps,
    map::{PerCpuVariants, PerCpuVariantsOps, UnifiedMap},
};
use starry_vm::{VmBytes, VmBytesMut};

use crate::{bpf::map::BpfMap, file::get_file_like, mm::vm_load_string, perf::perf_event_output};

pub fn bpferror_to_axresult(err: BpfError) -> AxResult<isize> {
    Err(bpferror_to_axerr(err))
}

pub fn bpferror_to_axerr(err: BpfError) -> AxError {
    match err {
        BpfError::InvalidArgument => AxError::InvalidInput,
        BpfError::NotFound => AxError::NotFound,
        BpfError::NotSupported => AxError::OperationNotSupported,
        BpfError::NoSpace => AxError::NoMemory,
    }
}

#[derive(Debug)]
pub struct PerCpuImpl;
impl PerCpuVariantsOps for PerCpuImpl {
    fn create<T: Clone + Sync + Send + 'static>(value: T) -> Option<Box<dyn PerCpuVariants<T>>> {
        let data = PerCpuVariantsImpl::new_with_value(value);
        Some(Box::new(data))
    }

    fn num_cpus() -> u32 {
        CPU_NUM as _
    }
}

pub struct PerCpuVariantsImpl<T> {
    data: Vec<T>,
}

impl<T: Send + Sync + Clone> PerCpuVariantsImpl<T> {
    pub fn new() -> Self {
        Self {
            data: Vec::with_capacity(CPU_NUM),
        }
    }

    pub fn new_with_value(value: T) -> Self {
        Self {
            data: vec![value; CPU_NUM as usize],
        }
    }
}

impl<T> Debug for PerCpuVariantsImpl<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PerCpuVariantsImpl").finish()
    }
}

impl<T: Send + Sync + Clone> PerCpuVariants<T> for PerCpuVariantsImpl<T> {
    fn get(&self) -> &T {
        &self.data[this_cpu_id()]
    }

    fn get_mut(&self) -> &mut T {
        unsafe { &mut (self as *const Self as *mut Self).as_mut().unwrap().data[this_cpu_id()] }
    }

    unsafe fn force_get(&self, cpu: u32) -> &T {
        &self.data[cpu as usize]
    }

    unsafe fn force_get_mut(&self, cpu: u32) -> &mut T {
        unsafe { &mut (self as *const Self as *mut Self).as_mut().unwrap().data[cpu as usize] }
    }
}

#[derive(Debug)]
pub struct EbpfKernelAuxiliary;
impl KernelAuxiliaryOps for EbpfKernelAuxiliary {
    fn get_unified_map_from_ptr<F, R>(ptr: *const u8, func: F) -> kbpf_basic::Result<R>
    where
        F: FnOnce(&mut UnifiedMap) -> kbpf_basic::Result<R>,
    {
        let map = unsafe { Arc::from_raw(ptr as *const BpfMap) };
        let mut unified_map = map.unified_map();
        let ret = func(&mut *unified_map);
        drop(unified_map);
        // avoid double free
        // log::error!("get_unified_map_from_ptr: ret: {:?}", ret);
        let _ = Arc::into_raw(map);
        ret
    }

    fn get_unified_map_from_fd<F, R>(map_fd: u32, func: F) -> kbpf_basic::Result<R>
    where
        F: FnOnce(&mut UnifiedMap) -> kbpf_basic::Result<R>,
    {
        // log::error!("get_unified_map_from_fd: map_fd: {}", map_fd);
        let file = get_file_like(map_fd as _).map_err(|_| BpfError::NotFound)?;
        let bpf_map = file.into_any().downcast::<BpfMap>().unwrap();
        let unified_map = &mut bpf_map.unified_map();
        func(unified_map)
    }

    fn get_unified_map_ptr_from_fd(map_fd: u32) -> kbpf_basic::Result<*const u8> {
        // log::error!("get_unified_map_ptr_from_fd: map_fd: {}", map_fd);
        let file = get_file_like(map_fd as _).map_err(|_| BpfError::NotFound)?;
        let bpf_map = file.into_any().downcast::<BpfMap>().unwrap();
        let map_ptr = Arc::into_raw(bpf_map) as usize;
        Ok(map_ptr as *const u8)
    }

    fn copy_from_user(src: *const u8, size: usize, dst: &mut [u8]) -> kbpf_basic::Result<()> {
        // TODO: remove unwrap
        let l = VmBytes::new(src, size).read(dst).unwrap();
        assert_eq!(l, size);
        Ok(())
    }

    fn copy_to_user(dest: *mut u8, size: usize, src: &[u8]) -> kbpf_basic::Result<()> {
        // TODO: remove unwrap
        let l = VmBytesMut::new(dest, size).write(src).unwrap();
        assert_eq!(l, size);
        Ok(())
    }

    fn current_cpu_id() -> u32 {
        this_cpu_id() as _
    }

    fn perf_event_output(
        ctx: *mut c_void,
        fd: u32,
        flags: u32,
        data: &[u8],
    ) -> kbpf_basic::Result<()> {
        perf_event_output(ctx, fd as usize, flags, data).map_err(|_| BpfError::InvalidArgument)
    }

    fn string_from_user_cstr(ptr: *const u8) -> kbpf_basic::Result<String> {
        let str = vm_load_string(ptr).map_err(|_| BpfError::InvalidArgument)?;
        // axlog::info!("string_from_user_cstr: string: {:?}", str);
        Ok(str)
    }

    fn ebpf_write_str(str: &str) -> kbpf_basic::Result<()> {
        axlog::info!("ebpf_write_str: str: {:?}", str);
        Ok(())
    }

    fn ebpf_time_ns() -> kbpf_basic::Result<u64> {
        Ok(monotonic_time_nanos())
    }
}
