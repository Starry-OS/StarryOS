use axerrno::AxResult;

use crate::task::{do_exit, encode_exit_status};

pub fn sys_exit(exit_code: i32) -> AxResult<isize> {
    let wait_status = encode_exit_status(exit_code as u8);
    do_exit(wait_status, false);
    Ok(0)
}

pub fn sys_exit_group(exit_code: i32) -> AxResult<isize> {
    let wait_status = encode_exit_status(exit_code as u8);
    do_exit(wait_status, true);
    Ok(0)
}
