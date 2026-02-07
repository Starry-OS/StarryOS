use alloc::{sync::Arc, vec::Vec};

use axerrno::{AxError, AxResult};
use linux_raw_sys::net::{SCM_RIGHTS, SOL_SOCKET, cmsghdr, socklen_t};
use starry_vm::{VmMutPtr, VmPtr, vm_load};

use crate::file::{FileLike, get_file_like};

pub enum CMsg {
    Rights { fds: Vec<Arc<dyn FileLike>> },
}
impl CMsg {
    pub fn parse(hdr: &cmsghdr) -> AxResult<Self> {
        if hdr.cmsg_len < size_of::<cmsghdr>() {
            return Err(AxError::InvalidInput);
        }

        let data_len = hdr.cmsg_len - size_of::<cmsghdr>();
        let data_ptr = (hdr as *const cmsghdr as usize + size_of::<cmsghdr>()) as *const u8;
        let data = vm_load(data_ptr, data_len)?;

        Ok(match (hdr.cmsg_level as u32, hdr.cmsg_type as u32) {
            (SOL_SOCKET, SCM_RIGHTS) => {
                if !data.len().is_multiple_of(size_of::<i32>()) {
                    return Err(AxError::InvalidInput);
                }
                let mut fds = Vec::new();
                for fd in data.chunks_exact(size_of::<i32>()) {
                    let fd = i32::from_ne_bytes(fd.try_into().unwrap());
                    if fd < 0 {
                        return Err(AxError::BadFileDescriptor);
                    }
                    let f = get_file_like(fd)?;
                    fds.push(f);
                }
                Self::Rights { fds }
            }
            _ => {
                return Err(AxError::InvalidInput);
            }
        })
    }
}

pub struct CMsgBuilder {
    hdr: *mut cmsghdr,
    len: *mut socklen_t,
    capacity: usize,
    written: usize,
}
impl CMsgBuilder {
    pub fn new(msg: *mut cmsghdr, len: *mut socklen_t) -> AxResult<Self> {
        let capacity = len.vm_read()? as usize;
        len.vm_write(0)?;
        Ok(Self {
            hdr: msg,
            len,
            capacity,
            written: 0,
        })
    }

    pub fn push(
        &mut self,
        level: u32,
        ty: u32,
        body: impl FnOnce(*mut u8, usize) -> AxResult<usize>,
    ) -> AxResult<bool> {
        let Some(body_capacity) = (self.capacity - self.written).checked_sub(size_of::<cmsghdr>())
        else {
            return Ok(false);
        };

        let data_ptr = ((self.hdr as usize) + size_of::<cmsghdr>()) as *mut u8;
        let body_len = body(data_ptr, body_capacity)?;

        let cmsg_len = size_of::<cmsghdr>() + body_len;
        let hdr = cmsghdr {
            cmsg_len,
            cmsg_level: level as _,
            cmsg_type: ty as _,
        };
        self.hdr.vm_write(hdr)?;

        self.hdr = (self.hdr as usize + cmsg_len) as *mut cmsghdr;
        self.written += cmsg_len;
        self.len.vm_write(self.written as socklen_t)?;
        Ok(true)
    }
}
