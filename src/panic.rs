use core::panic::PanicInfo;

#[cfg(not(target_arch = "loongarch64"))]
#[allow(unused)]
pub use unwind::{KPanicInfo, catch_panics_as_oops};

#[cfg(target_arch = "loongarch64")]
#[axmacros::panic_handler]
fn panic(info: &PanicInfo) -> ! {
    ax_println!("{}", info);
    axhal::power::system_off()
}

#[cfg(not(target_arch = "loongarch64"))]
mod unwind {
    use alloc::boxed::Box;
    use core::{
        ffi::c_void,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use unwinding::{
        abi::{
            _Unwind_Backtrace, _Unwind_FindEnclosingFunction, _Unwind_GetIP, UnwindContext,
            UnwindReasonCode,
        },
        panic,
    };

    use super::PanicInfo;

    static RECURSION: AtomicBool = AtomicBool::new(false);
    #[derive(Debug)]
    pub struct KPanicInfo;

    impl KPanicInfo {
        pub fn new() -> Self {
            Self
        }
    }

    #[axmacros::panic_handler]
    fn panic_handler(info: &PanicInfo) -> ! {
        if let Some(p) = info.location() {
            ax_println!("line {}, file {}: {}", p.line(), p.file(), info.message());
        } else {
            ax_println!("no location information available");
        }
        if !RECURSION.swap(true, core::sync::atomic::Ordering::SeqCst) {
            if info.can_unwind() {
                let guard = Box::new(KPanicInfo::new());
                print_stack_trace();
                let _res = unwinding::panic::begin_panic(guard);
                panic!("panic unreachable: {:?}", _res.0);
            }
        }
        axhal::power::system_off()
    }

    pub fn print_stack_trace() {
        ax_println!("Rust Panic Backtrace:");
        struct CallbackData {
            counter: usize,
            kernel_main: bool,
        }
        extern "C" fn callback(
            unwind_ctx: &UnwindContext<'_>,
            arg: *mut c_void,
        ) -> UnwindReasonCode {
            let data = unsafe { &mut *(arg as *mut CallbackData) };
            if data.kernel_main {
                // If we are in kernel_main, we don't need to print the backtrace.
                return UnwindReasonCode::NORMAL_STOP;
            }
            data.counter += 1;
            let pc = _Unwind_GetIP(unwind_ctx);
            if pc > 0 {
                let fde_initial_address = _Unwind_FindEnclosingFunction(pc as *mut c_void) as usize;
                // TODO: lookup_kallsyms
                ax_println!(
                    "#{:<2} {:#018x} - <unknown> + {:#x}",
                    data.counter,
                    pc,
                    pc - fde_initial_address
                );
            }
            UnwindReasonCode::NO_REASON
        }
        let mut data = CallbackData {
            counter: 0,
            kernel_main: false,
        };
        _Unwind_Backtrace(callback, &mut data as *mut _ as _);
    }

    /// The maximum number of oops allowed before the kernel panics.
    ///
    /// It is the same as Linux's default value.
    const MAX_OOPS_COUNT: usize = 10_000;

    static OOPS_COUNT: AtomicUsize = AtomicUsize::new(0);

    /// Catch panics in the given closure and treat them as kernel oops.
    pub fn catch_panics_as_oops<F, R>(f: F) -> Result<R, KPanicInfo>
    where
        F: FnOnce() -> R,
    {
        let result = panic::catch_unwind(f);

        match result {
            Ok(result) => Ok(result),
            Err(err) => {
                let info = err.downcast::<KPanicInfo>().unwrap();

                let count = OOPS_COUNT.fetch_add(1, Ordering::Relaxed);
                if count >= MAX_OOPS_COUNT {
                    // Too many oops. Abort the kernel.
                    axlog::error!("Too many oops. The kernel panics.");
                    axhal::power::system_off();
                }
                Err(*info)
            }
        }
    }
}

#[cfg(not(target_arch = "loongarch64"))]
pub fn test_unwind() {
    struct UnwindTest;
    impl Drop for UnwindTest {
        fn drop(&mut self) {
            ax_println!("Drop UnwindTest");
        }
    }
    let res1 = catch_panics_as_oops(|| {
        let _unwind_test = UnwindTest;
        ax_println!("Test panic...");
        panic!("Test panic");
    });
    assert!(res1.is_err());
    let res2 = catch_panics_as_oops(|| {
        let _unwind_test = UnwindTest;
        ax_println!("Test no panic...");
        0
    });
    assert!(res2.is_ok());
    ax_println!("Unwind test passed.");
}
