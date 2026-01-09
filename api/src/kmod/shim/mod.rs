use core::ptr::null_mut;

use kmod::capi_fn;

/// ASCII SOH character.
///
/// See <https://elixir.bootlin.com/linux/v6.17/source/include/linux/kern_levels.h#L5>
#[allow(dead_code)]
const KERN_SOH: u8 = 0x01;

const KERN_EMERG: &[u8; 2] = concat_bytes!(b'\x01', b'0');
const KERN_ALERT: &[u8; 2] = concat_bytes!(b'\x01', b'1');
const KERN_CRIT: &[u8; 2] = concat_bytes!(b'\x01', b'2');
const KERN_ERR: &[u8; 2] = concat_bytes!(b'\x01', b'3');
const KERN_WARNING: &[u8; 2] = concat_bytes!(b'\x01', b'4');
const KERN_NOTICE: &[u8; 2] = concat_bytes!(b'\x01', b'5');
const KERN_INFO: &[u8; 2] = concat_bytes!(b'\x01', b'6');
const KERN_DEBUG: &[u8; 2] = concat_bytes!(b'\x01', b'7');

const LOG_LEVELS: &[&[u8; 2]] = &[
    KERN_EMERG,
    KERN_ALERT,
    KERN_CRIT,
    KERN_ERR,
    KERN_WARNING,
    KERN_NOTICE,
    KERN_INFO,
    KERN_DEBUG,
];

#[capi_fn]
unsafe extern "C" fn write_char(c: u8) {
    ax_print!("{}", c as char);
}

#[capi_fn]
unsafe extern "C" fn _printk(fmt: *const u8, mut args: ...) -> i32 {
    let c_str_fmt = unsafe { core::ffi::CStr::from_ptr(fmt as *const _) };
    let fmt_bytes = c_str_fmt.to_bytes();
    let level_prefix = LOG_LEVELS
        .iter()
        .find(|&&level| fmt_bytes.starts_with(level))
        .copied();
    let fmt = if let Some(level) = level_prefix {
        &fmt_bytes[level.len()..]
    } else {
        fmt_bytes
    };
    match level_prefix {
        Some(KERN_EMERG) | Some(KERN_ALERT) | Some(KERN_CRIT) | Some(KERN_ERR) => {
            ax_print!("[ERROR] ");
        }
        Some(KERN_WARNING) => {
            ax_print!("[WARN] ");
        }
        Some(KERN_NOTICE) | Some(KERN_INFO) => {
            ax_print!("[INFO] ");
        }
        Some(KERN_DEBUG) => {
            ax_print!("[DEBUG] ");
        }
        _ => {
            ax_print!("[INFO] ");
        }
    }
    unsafe {
        lwprintf_rs::lwprintf_vprintf_ex_rust(null_mut(), fmt.as_ptr() as _, args.as_va_list())
    }
}
