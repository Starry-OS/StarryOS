use axcpu::trap::BREAK_HANDLER;
use axhal::context::TrapFrame;
use linkme::distributed_slice;
#[distributed_slice(BREAK_HANDLER)]
// static BENCH_DESERIALIZE: fn(&mut TrapFrame) -> bool = ebreak_handler;

// break 异常处理
pub fn ebreak_handler(tf: &mut TrapFrame) -> bool {
    // ax_println!("ebreak_handler from kernel");
    let res = crate::kprobe::run_all_kprobe(tf);
    if res.is_some() {
        // if kprobe is hit, the spec will be updated in kprobe_handler
        return true;
    }
	#[cfg(target_arch = "riscv64")]
    {
		tf.sepc += 2;
	}
	#[cfg(target_arch = "loongarch64")]
	{
		tf.era +=4;
	}
	true
}


