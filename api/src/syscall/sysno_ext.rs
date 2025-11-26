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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_standard_syscall() {
        // Test that standard syscalls are correctly wrapped
        if let Some(sysno_ext) = SysnoExt::new(0) {
            // syscall 0 should be a standard syscall (io_setup on RISC-V)
            assert!(matches!(sysno_ext, SysnoExt::Standard(_)));
            assert!(sysno_ext.to_standard().is_some());
        }
    }

    #[cfg(target_arch = "riscv64")]
    #[test]
    fn test_new_riscv_hwprobe() {
        // Test that syscall 258 is recognized as RiscvHwprobe
        let sysno_ext = SysnoExt::new(258).expect("Should recognize riscv_hwprobe");
        assert!(matches!(sysno_ext, SysnoExt::RiscvHwprobe));
        assert!(sysno_ext.to_standard().is_none());
    }

    #[cfg(target_arch = "riscv64")]
    #[test]
    fn test_new_riscv_flush_icache() {
        // Test that syscall 259 is recognized as RiscvFlushIcache
        let sysno_ext = SysnoExt::new(259).expect("Should recognize riscv_flush_icache");
        assert!(matches!(sysno_ext, SysnoExt::RiscvFlushIcache));
        assert!(sysno_ext.to_standard().is_none());
    }

    #[test]
    fn test_new_invalid_syscall() {
        // Test that invalid syscall numbers return None
        // Use a very large number that's unlikely to be a valid syscall
        assert!(SysnoExt::new(999999).is_none());
    }

    #[test]
    fn test_from_sysno() {
        // Test From<Sysno> implementation
        if let Some(sysno) = Sysno::new(0) {
            let sysno_ext: SysnoExt = sysno.into();
            assert!(matches!(sysno_ext, SysnoExt::Standard(_)));
            assert_eq!(sysno_ext.to_standard(), Some(sysno));
        }
    }

    #[test]
    fn test_to_standard() {
        // Test conversion to standard Sysno
        if let Some(sysno) = Sysno::new(1) {
            let sysno_ext = SysnoExt::Standard(sysno);
            assert_eq!(sysno_ext.to_standard(), Some(sysno));
        }
    }

    #[cfg(target_arch = "riscv64")]
    #[test]
    fn test_riscv_specific_to_standard() {
        // Test that RISC-V specific syscalls cannot be converted to standard
        let hwprobe = SysnoExt::RiscvHwprobe;
        assert!(hwprobe.to_standard().is_none());

        let flush_icache = SysnoExt::RiscvFlushIcache;
        assert!(flush_icache.to_standard().is_none());
    }
}