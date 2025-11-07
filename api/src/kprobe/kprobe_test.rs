use alloc::string::ToString;

use kprobe::{KprobeBuilder, KretprobeBuilder, ProbeData, PtRegs};

use crate::{
    kprobe::{register_kprobe, unregister_kprobe},
    lock_api::KSpinNoPreempt,
};

#[inline(never)]
#[unsafe(no_mangle)]
fn detect_func(x: usize, y: usize, z: Option<usize>) -> Option<usize> {
    let hart = 0;
    ax_println!("detect_func: hart_id: {}, x: {}, y:{}", hart, x, y);
    if let Some(z) = z {
        Some(x + y + z)
    } else {
        None
    }
}

fn pre_handler(_data: &dyn ProbeData, pt_regs: &mut PtRegs) {
    ax_println!(
        "[kprobe] pre_handler: ret_value: {}",
        pt_regs.first_ret_value()
    );
}

fn post_handler(_data: &dyn ProbeData, pt_regs: &mut PtRegs) {
    ax_println!(
        "[kprobe] post_handler: ret_value: {}",
        pt_regs.first_ret_value()
    );
}

pub fn kprobe_test() {
    ax_println!(
        "[kprobe] kprobe test for [detect_func]: {:#x}",
        detect_func as usize
    );
    let kprobe_builder = KprobeBuilder::new(None, detect_func as usize, 0, true)
        .with_pre_handler(pre_handler)
        .with_post_handler(post_handler);

    let kprobe = register_kprobe(kprobe_builder);
    let new_pre_handler = |_data: &dyn ProbeData, pt_regs: &mut PtRegs| {
        ax_println!(
            "[kprobe] new_pre_handler: ret_value: {}",
            pt_regs.first_ret_value()
        );
    };

    let builder2 = KprobeBuilder::new(
        Some("kprobe::detect_func".to_string()),
        detect_func as usize,
        0,
        true,
    )
    .with_pre_handler(new_pre_handler)
    .with_post_handler(post_handler);

    let kprobe2 = register_kprobe(builder2);
    ax_println!(
        "[kprobe] install 2 kprobes at [detect_func]: {:#x}",
        detect_func as usize
    );
    detect_func(1, 2, Some(3));

    unregister_kprobe(kprobe);
    unregister_kprobe(kprobe2);
    ax_println!(
        "[kprobe] uninstall 2 kprobes at [detect_func]: {:#x}",
        detect_func as usize
    );

    let kretprobe_builder =
        KretprobeBuilder::<KSpinNoPreempt<()>>::new(None, detect_func as usize, 10)
            .with_ret_handler(kret_post_handler);

    let kretprobe = crate::kprobe::register_kretprobe(kretprobe_builder);
    ax_println!(
        "[kretprobe] install kretprobe at [detect_func]: {:#x}",
        detect_func as usize
    );
    detect_func(0xff, 0, Some(1));

    crate::kprobe::unregister_kretprobe(kretprobe);

    detect_func(3, 4, None);
    ax_println!("[kprobe] [kretprobe] test passed");
}

fn kret_post_handler(_data: &dyn ProbeData, pt_regs: &mut PtRegs) {
    ax_println!(
        "[kretprobe] post_handler: ret_value(a0): {}, ret_value(a1): {}",
        pt_regs.first_ret_value(),
        pt_regs.second_ret_value()
    );
}
