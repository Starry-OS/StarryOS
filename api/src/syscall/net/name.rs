use axerrno::AxResult;
use axnet::SocketOps;
use linux_raw_sys::net::{sockaddr, socklen_t};
use starry_vm::{VmMutPtr, VmPtr};

use crate::{
    file::{FileLike, Socket},
    socket::SocketAddrExt,
};

pub fn sys_getsockname(fd: i32, addr: *mut sockaddr, addrlen: *mut socklen_t) -> AxResult<isize> {
    let socket = Socket::from_fd(fd)?;
    let local_addr = socket.local_addr()?;
    debug!("sys_getsockname <= fd: {fd}, addr: {local_addr:?}");

    let mut len = addrlen.vm_read()?;
    local_addr.write_to_user(addr, &mut len)?;
    addrlen.vm_write(len)?;
    Ok(0)
}

pub fn sys_getpeername(fd: i32, addr: *mut sockaddr, addrlen: *mut socklen_t) -> AxResult<isize> {
    let socket = Socket::from_fd(fd)?;
    let peer_addr = socket.peer_addr()?;
    debug!("sys_getpeername <= fd: {fd}, addr: {peer_addr:?}");

    let mut len = addrlen.vm_read()?;
    peer_addr.write_to_user(addr, &mut len)?;
    addrlen.vm_write(len)?;
    Ok(0)
}
