use std::{fs::File, io::{self, Read}, mem, os::unix::io::{FromRawFd, RawFd}, ptr};

fn main() -> io::Result<()> {
    let mask = unsafe { build_sigmask()? };
    block_signals(&mask).map_err(|e| {
        eprintln!("sigprocmask failed: {e}");
        e
    })?;
    let fd = create_signalfd(&mask).map_err(|e| {
        eprintln!("signalfd4 failed: {e}");
        e
    })?;
    println!("signalfd created: fd = {fd}");

    // 触发一个 SIGUSR1，确保 signalfd 有数据可读
    send_signal(libc::SIGUSR1).map_err(|e| {
        eprintln!("raise(SIGUSR1) failed: {e}");
        e
    })?;
    read_signalfd(fd).map_err(|e| {
        eprintln!("read(signalfd) failed: {e}");
        e
    })?;
    Ok(())
}

unsafe fn build_sigmask() -> io::Result<libc::sigset_t> {
    let mut mask: libc::sigset_t = mem::zeroed();
    if libc::sigemptyset(&mut mask) != 0 {
        return Err(io::Error::last_os_error());
    }
    for sig in [libc::SIGUSR1, libc::SIGUSR2] {
        if libc::sigaddset(&mut mask, sig) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(mask)
}

fn block_signals(mask: &libc::sigset_t) -> io::Result<()> {
    let ret = unsafe { libc::sigprocmask(libc::SIG_BLOCK, mask, ptr::null_mut()) };
    if ret != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn create_signalfd(mask: &libc::sigset_t) -> io::Result<RawFd> {
    let candidates = [mem::size_of::<libc::sigset_t>(), 0];
    let mut last_err = None;
    for &sigset_size in &candidates {
        let fd = unsafe {
            libc::syscall(
                libc::SYS_signalfd4,
                -1i32,
                mask as *const libc::sigset_t,
                sigset_size,
                libc::SFD_CLOEXEC,
            ) as RawFd
        };
        if fd >= 0 {
            return Ok(fd);
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::InvalidInput {
            return Err(err);
        }
        last_err = Some(err);
    }
    Err(last_err.unwrap_or_else(io::Error::last_os_error))
}

fn send_signal(sig: libc::c_int) -> io::Result<()> {
    let ret = unsafe { libc::raise(sig) };
    if ret != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn read_signalfd(fd: RawFd) -> io::Result<()> {
    let mut file = unsafe { File::from_raw_fd(fd) };
    let mut buffer = [0u8; 128];
    match file.read(&mut buffer) {
        Ok(n) => println!("read {n} bytes from signalfd"),
        Err(e) => return Err(e),
    }
    Ok(())
}
