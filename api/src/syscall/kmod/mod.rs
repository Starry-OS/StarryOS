use alloc::vec;

use axerrno::AxResult;
use axio::Read;

use crate::mm::{VmBytes, vm_load_string};

/// See <https://man7.org/linux/man-pages/man2/init_module.2.html>
pub fn sys_init_module(module_ptr: *const u8, len: usize, param_ptr: *const u8) -> AxResult<isize> {
    let mut module_buf = VmBytes::new(module_ptr as *mut u8, len);
    let mut module_data = vec![0u8; len];
    module_buf.read(&mut module_data)?;

    let param_buf = if !param_ptr.is_null() {
        Some(vm_load_string(param_ptr as _)?)
    } else {
        None
    };

    axlog::warn!(
        "[sys_init_module]: module_len={}, params={:?}",
        len,
        param_buf
    );

    crate::kmod::init_module(&module_data, param_buf.as_deref())?;
    Ok(0)
}


/// See<https://man7.org/linux/man-pages/man2/delete_module.2.html>
pub fn sys_delete_module(name_ptr: *const u8, _flags: u32) -> AxResult<isize> {
    let name = vm_load_string(name_ptr as _)?;

    axlog::warn!("[sys_delete_module]: name={}", name);

    crate::kmod::delete_module(&name)?;
    Ok(0)
}