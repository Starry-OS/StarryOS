use axhal::context::TrapFrame;

/// Convert a TrapFrame to kprobe::PtRegs
pub fn tf_to_ptregs(tf: &TrapFrame) -> kprobe::PtRegs {
    kprobe::PtRegs {
        r15: tf.r15 as _,
        r14: tf.r14 as _,
        r13: tf.r13 as _,
        r12: tf.r12 as _,
        rbp: tf.rbp as _,
        rbx: tf.rbx as _,
        r11: tf.r11 as _,
        r10: tf.r10 as _,
        r9: tf.r9 as _,
        r8: tf.r8 as _,
        rax: tf.rax as _,
        rcx: tf.rcx as _,
        rdx: tf.rdx as _,
        rsi: tf.rsi as _,
        rdi: tf.rdi as _,
        orig_rax: tf.vector as _,
        rip: tf.rip as _,
        cs: tf.cs as _,
        rsp: tf.rsp as _,
        ss: tf.ss as _,
        rflags: tf.rflags as _,
    }
}

/// Update the TrapFrame from kprobe::PtRegs
pub fn ptregs_to_tf(ptregs: kprobe::PtRegs, tf: &mut TrapFrame) {
    tf.r15 = ptregs.r15 as _;
    tf.r14 = ptregs.r14 as _;
    tf.r13 = ptregs.r13 as _;
    tf.r12 = ptregs.r12 as _;
    tf.rbp = ptregs.rbp as _;
    tf.rbx = ptregs.rbx as _;
    tf.r11 = ptregs.r11 as _;
    tf.r10 = ptregs.r10 as _;
    tf.r9 = ptregs.r9 as _;
    tf.r8 = ptregs.r8 as _;
    tf.rax = ptregs.rax as _;
    tf.rcx = ptregs.rcx as _;
    tf.rdx = ptregs.rdx as _;
    tf.rsi = ptregs.rsi as _;
    tf.rdi = ptregs.rdi as _;
    tf.vector = ptregs.orig_rax as _;
    tf.rip = ptregs.rip as _;
    tf.cs = ptregs.cs as _;
    tf.rflags = ptregs.rflags as _;
    tf.rsp = ptregs.rsp as _;
    tf.ss = ptregs.ss as _;
}
