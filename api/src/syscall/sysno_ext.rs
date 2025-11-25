//! Extension for syscalls::Sysno to include missing RISC-V specific syscalls
//!
//! This module provides an extended Sysno enum that includes riscv_hwprobe (258)
//! and riscv_flush_icache (259) which are missing from syscalls crate 0.7.0
//! due to Linux v6.11 header structure changes.
//!
//! See: https://github.com/jasonwhite/syscalls/issues/58
//! See also: docs/syscalls-riscv-issue.md

use syscalls::Sysno;

/// Extended system call number that includes RISC-V specific syscalls
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SysnoExt {
    /// Standard syscall from syscalls crate
    Standard(Sysno),
    /// RISC-V specific: riscv_hwprobe (258)
    #[cfg(target_arch = "riscv64")]
    RiscvHwprobe,
    /// RISC-V specific: riscv_flush_icache (259)
    #[cfg(target_arch = "riscv64")]
    RiscvFlushIcache,
}

impl SysnoExt {
    /// Create a SysnoExt from a raw system call number
    pub fn new(sysno: usize) -> Option<Self> {
        #[cfg(target_arch = "riscv64")]
        {
            match sysno {
                258 => return Some(SysnoExt::RiscvHwprobe),
                259 => return Some(SysnoExt::RiscvFlushIcache),
                _ => {}
            }
        }
        
        Sysno::new(sysno).map(SysnoExt::Standard)
    }

    /// Get the raw system call number
    pub fn as_usize(self) -> usize {
        match self {
            SysnoExt::Standard(sysno) => sysno as usize,
            #[cfg(target_arch = "riscv64")]
            SysnoExt::RiscvHwprobe => 258,
            #[cfg(target_arch = "riscv64")]
            SysnoExt::RiscvFlushIcache => 259,
        }
    }

    /// Get the raw system call number as u64
    pub fn as_u64(self) -> u64 {
        self.as_usize() as u64
    }

    /// Check if this is a RISC-V specific system call
    #[cfg(target_arch = "riscv64")]
    pub fn is_riscv_specific(self) -> bool {
        matches!(self, SysnoExt::RiscvHwprobe | SysnoExt::RiscvFlushIcache)
    }

    /// Convert to standard Sysno if possible
    pub fn to_standard(self) -> Option<Sysno> {
        match self {
            SysnoExt::Standard(sysno) => Some(sysno),
            #[cfg(target_arch = "riscv64")]
            _ => None,
        }
    }
}

impl From<Sysno> for SysnoExt {
    fn from(sysno: Sysno) -> Self {
        SysnoExt::Standard(sysno)
    }
}

