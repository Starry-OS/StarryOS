use axhal::context::TrapFrame;

/// Convert a TrapFrame to kprobe::PtRegs
pub fn tf_to_ptregs(tf: &TrapFrame) -> kprobe::PtRegs {
    let regs = [0; 32];
    unsafe {
        core::ptr::copy_nonoverlapping(
            &tf.regs as *const _ as *const usize,
            regs.as_ptr() as *mut usize,
            32,
        );
    }
    kprobe::PtRegs {
        regs,
        orig_a0: 0,
        csr_era: tf.era,
        csr_badvaddr: 0,
        csr_crmd: 0,
        csr_prmd: tf.prmd,
        csr_euen: 0,
        csr_ecfg: 0,
        csr_estat: 0,
    }
}

/// Update the TrapFrame from kprobe::PtRegs
pub fn ptregs_to_tf(ptregs: kprobe::PtRegs, tf: &mut TrapFrame) {
    unsafe {
        core::ptr::copy_nonoverlapping(
            ptregs.regs.as_ptr() as *const usize,
            &mut tf.regs as *mut _ as *mut usize,
            32,
        );
    }
    tf.era = ptregs.csr_era;
    tf.prmd = ptregs.csr_prmd;
}
