use axhal::context::TrapFrame;
use riscv::register::sstatus;

/// Convert a TrapFrame to kprobe::PtRegs
pub fn tf_to_ptregs(tf: &TrapFrame) -> kprobe::PtRegs {
    kprobe::PtRegs {
        epc: tf.sepc,
        ra: tf.regs.ra,
        sp: tf.regs.sp,
        gp: tf.regs.gp,
        tp: tf.regs.tp,
        t0: tf.regs.t0,
        t1: tf.regs.t1,
        t2: tf.regs.t2,
        s0: tf.regs.s0,
        s1: tf.regs.s1,
        a0: tf.regs.a0,
        a1: tf.regs.a1,
        a2: tf.regs.a2,
        a3: tf.regs.a3,
        a4: tf.regs.a4,
        a5: tf.regs.a5,
        a6: tf.regs.a6,
        a7: tf.regs.a7,
        s2: tf.regs.s2,
        s3: tf.regs.s3,
        s4: tf.regs.s4,
        s5: tf.regs.s5,
        s6: tf.regs.s6,
        s7: tf.regs.s7,
        s8: tf.regs.s8,
        s9: tf.regs.s9,
        s10: tf.regs.s10,
        s11: tf.regs.s11,
        t3: tf.regs.t3,
        t4: tf.regs.t4,
        t5: tf.regs.t5,
        t6: tf.regs.t6,
        status: tf.sstatus.bits(),
        // todo : other fields
        badaddr: 0,
        cause: 0,
        orig_a0: 0,
    }
}

/// Update the TrapFrame from kprobe::PtRegs
pub fn ptregs_to_tf(ptregs: kprobe::PtRegs, tf: &mut TrapFrame) {
    tf.sepc = ptregs.epc;
    tf.regs.ra = ptregs.ra;
    tf.regs.sp = ptregs.sp;
    tf.regs.gp = ptregs.gp;
    tf.regs.tp = ptregs.tp;
    tf.regs.t0 = ptregs.t0;
    tf.regs.t1 = ptregs.t1;
    tf.regs.t2 = ptregs.t2;
    tf.regs.s0 = ptregs.s0;
    tf.regs.s1 = ptregs.s1;
    tf.regs.a0 = ptregs.a0;
    tf.regs.a1 = ptregs.a1;
    tf.regs.a2 = ptregs.a2;
    tf.regs.a3 = ptregs.a3;
    tf.regs.a4 = ptregs.a4;
    tf.regs.a5 = ptregs.a5;
    tf.regs.a6 = ptregs.a6;
    tf.regs.a7 = ptregs.a7;
    tf.regs.s2 = ptregs.s2;
    tf.regs.s3 = ptregs.s3;
    tf.regs.s4 = ptregs.s4;
    tf.regs.s5 = ptregs.s5;
    tf.regs.s6 = ptregs.s6;
    tf.regs.s7 = ptregs.s7;
    tf.regs.s8 = ptregs.s8;
    tf.regs.s9 = ptregs.s9;
    tf.regs.s10 = ptregs.s10;
    tf.regs.s11 = ptregs.s11;
    tf.regs.t3 = ptregs.t3;
    tf.regs.t4 = ptregs.t4;
    tf.regs.t5 = ptregs.t5;
    tf.regs.t6 = ptregs.t6;
    tf.sstatus = sstatus::Sstatus::from_bits(ptregs.status);
}
