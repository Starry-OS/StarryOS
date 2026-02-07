use alloc::{boxed::Box, vec::Vec};
use core::{net::Ipv4Addr, ptr::addr_of_mut};

use axerrno::{AxError, AxResult};
use axio::prelude::*;
use axnet::{CMsgData, RecvFlags, RecvOptions, SendFlags, SendOptions, SocketAddrEx, SocketOps};
use linux_raw_sys::net::{
    MSG_PEEK, MSG_TRUNC, SCM_RIGHTS, SOL_SOCKET, cmsghdr, msghdr, sockaddr, socklen_t,
};
use starry_vm::{VmMutPtr, VmPtr};

use crate::{
    file::{FileLike, Socket, add_file_like},
    io::{IoVec, IoVectorBuf},
    mm::{VmBytes, VmBytesMut},
    socket::SocketAddrExt,
    syscall::net::{CMsg, CMsgBuilder},
};

fn send_impl(
    fd: i32,
    mut src: impl Read + IoBuf,
    flags: u32,
    addr: *const sockaddr,
    addrlen: socklen_t,
    cmsg: Vec<CMsgData>,
) -> AxResult<isize> {
    let addr = if addr.is_null() || addrlen == 0 {
        None
    } else {
        Some(SocketAddrEx::read_from_user(addr, addrlen)?)
    };

    debug!("sys_send <= fd: {fd}, flags: {flags}, addr: {addr:?}");

    let socket = Socket::from_fd(fd)?;
    let sent = socket.send(
        &mut src,
        SendOptions {
            to: addr,
            flags: SendFlags::default(),
            cmsg,
        },
    )?;

    Ok(sent as isize)
}

pub fn sys_sendto(
    fd: i32,
    buf: *const u8,
    len: usize,
    flags: u32,
    addr: *const sockaddr,
    addrlen: socklen_t,
) -> AxResult<isize> {
    send_impl(fd, VmBytes::new(buf, len), flags, addr, addrlen, Vec::new())
}

pub fn sys_sendmsg(fd: i32, msg: *const msghdr, flags: u32) -> AxResult<isize> {
    let msg = unsafe { msg.vm_read_uninit()?.assume_init() };
    let mut cmsg = Vec::new();
    if !msg.msg_control.is_null() {
        let mut ptr = msg.msg_control as usize;
        let ptr_end = ptr + msg.msg_controllen;
        while ptr + size_of::<cmsghdr>() <= ptr_end {
            let hdr = unsafe { (ptr as *const cmsghdr).vm_read_uninit()?.assume_init() };
            if ptr_end - ptr < hdr.cmsg_len {
                return Err(AxError::InvalidInput);
            }
            cmsg.push(Box::new(CMsg::parse(&hdr)?) as CMsgData);
            ptr += hdr.cmsg_len;
        }
    }
    send_impl(
        fd,
        IoVectorBuf::new(msg.msg_iov as *const IoVec, msg.msg_iovlen)?.into_io(),
        flags,
        msg.msg_name as *const sockaddr,
        msg.msg_namelen as socklen_t,
        cmsg,
    )
}

fn recv_impl(
    fd: i32,
    mut dst: impl Write + IoBufMut,
    flags: u32,
    addr: *mut sockaddr,
    addrlen: *mut socklen_t,
    cmsg_builder: Option<CMsgBuilder>,
) -> AxResult<isize> {
    debug!("sys_recv <= fd: {fd}, flags: {flags}");

    let socket = Socket::from_fd(fd)?;
    let mut recv_flags = RecvFlags::empty();
    if flags & MSG_PEEK != 0 {
        recv_flags |= RecvFlags::PEEK;
    }
    if flags & MSG_TRUNC != 0 {
        recv_flags |= RecvFlags::TRUNCATE;
    }

    let mut cmsg = Vec::new();

    let mut remote_addr =
        (!addr.is_null()).then(|| SocketAddrEx::Ip((Ipv4Addr::UNSPECIFIED, 0).into()));
    let recv = socket.recv(
        &mut dst,
        RecvOptions {
            from: remote_addr.as_mut(),
            flags: recv_flags,
            cmsg: Some(&mut cmsg),
        },
    )?;

    if let Some(remote_addr) = remote_addr {
        remote_addr.write_to_user(addr, addrlen)?;
    }

    if let Some(mut builder) = cmsg_builder {
        for cmsg in cmsg {
            let Ok(cmsg) = cmsg.downcast::<CMsg>() else {
                warn!("received unexpected cmsg");
                continue;
            };

            let pushed = match *cmsg {
                CMsg::Rights { fds } => {
                    builder.push(SOL_SOCKET, SCM_RIGHTS, |data_ptr, capacity| {
                        let mut written = 0;
                        for f in fds.into_iter() {
                            if written + size_of::<i32>() > capacity {
                                break;
                            }
                            let fd = add_file_like(f, false)?;
                            let ptr = ((data_ptr as usize) + written) as *mut i32;
                            ptr.vm_write(fd)?;
                            written += size_of::<i32>();
                        }
                        Ok(written)
                    })?
                }
            };
            if !pushed {
                break;
            }
        }
    }

    debug!("sys_recv => fd: {fd}, recv: {recv}");
    Ok(recv as isize)
}

pub fn sys_recvfrom(
    fd: i32,
    buf: *mut u8,
    len: usize,
    flags: u32,
    addr: *mut sockaddr,
    addrlen: *mut socklen_t,
) -> AxResult<isize> {
    recv_impl(fd, VmBytesMut::new(buf, len), flags, addr, addrlen, None)
}

pub fn sys_recvmsg(fd: i32, msg: *mut msghdr, flags: u32) -> AxResult<isize> {
    let msg_val = unsafe { msg.vm_read_uninit()?.assume_init() };

    recv_impl(
        fd,
        IoVectorBuf::new(msg_val.msg_iov as *mut IoVec, msg_val.msg_iovlen)?.into_io(),
        flags,
        msg_val.msg_name as *mut sockaddr,
        unsafe { addr_of_mut!((*msg).msg_namelen) } as *mut socklen_t,
        Option::from(msg_val.msg_control as *mut cmsghdr)
            .map(|control| {
                CMsgBuilder::new(control, unsafe { addr_of_mut!((*msg).msg_controllen) }
                    as *mut socklen_t)
            })
            .transpose()?,
    )
}
