use axhal::context::TrapFrame;

/// Convert a TrapFrame to kprobe::PtRegs
pub fn tf_to_ptregs(tf: &TrapFrame) -> kprobe::PtRegs {
    kprobe::PtRegs {
        regs: tf.x,
        sp: 0,
        pc: tf.elr,
        pstate: tf.spsr,
        orig_x0: 0,
        syscallno: 0,
        unused2: 0,
    }
}

/// Update the TrapFrame from kprobe::PtRegs
pub fn ptregs_to_tf(ptregs: kprobe::PtRegs, tf: &mut TrapFrame) {
    tf.x = ptregs.regs;
    tf.elr = ptregs.pc;
    tf.spsr = ptregs.pstate;
}
