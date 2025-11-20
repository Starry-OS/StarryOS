//! vDSO data management.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use axerrno::{AxError, AxResult};
use axhal::{
    mem::virt_to_phys,
    paging::MappingFlags,
    time::{NANOS_PER_SEC, current_ticks, monotonic_time_nanos, wall_time_nanos, nanos_to_ticks},
};
use axmm::AddrSpace;
use kernel_elf_parser::AuxEntry;
use memory_addr::{MemoryAddr, PAGE_SIZE_4K};

/// Clock mode constants
const VDSO_CLOCKMODE_NONE: i32 = 0;
const VDSO_CLOCKMODE_TSC: i32 = 1;
const VDSO_CLOCKMODE_PVCLOCK: i32 = 2;

/// Number of clock bases
const VDSO_BASES: usize = 16;

use core::arch::global_asm;

global_asm!("
    .global vdso_start, vdso_end
    .section .data
    .balign 4096
vdso_start:
    .incbin \"vdso.so\"
    .balign 4096
vdso_end:

    .previous
");

/// Compute multiplier and shift to convert from timer_frequency to nanos_per_sec.
fn clocks_calc_mult_shift(from: u64, to: u64, maxsec: u32) -> (u32, u32) {
    // sftacc starts at 32 and is reduced based on the maximum conversion range
    let mut tmp = ((maxsec as u64).wrapping_mul(from)) >> 32;
    let mut sftacc: i32 = 32;
    while tmp != 0 {
        tmp >>= 1;
        sftacc -= 1;
    }

    // Try shifts from 32 down to 1 and pick the first that fits the range
    for sft in (1..=32).rev() {
        // compute tmp = (to << sft) / from with rounding
        let mut numer = (to as u128) << sft;
        numer += (from as u128) / 2u128;
        let tmp128 = numer / (from as u128);

        // If tmp128 can be represented within the allowed shift range, select it
        if sftacc <= 0 || (tmp128 >> (sftacc as u128)) == 0u128 {
            let mult = if tmp128 > (u32::MAX as u128) {
                u32::MAX
            } else {
                tmp128 as u32
            };
            return (mult, sft as u32);
        }
    }
    // Fallback: return maximum multiplier with shift 0
    (u32::MAX, 0)
}

/// vDSO timestamp structure
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VdsoTimestamp {
    /// Seconds
    pub sec: u64,
    /// Nanoseconds
    pub nsec: u64,
}

impl VdsoTimestamp {
    /// Create a new zero timestamp
    pub const fn new() -> Self {
        Self { sec: 0, nsec: 0 }
    }
}

impl Default for VdsoTimestamp {
    fn default() -> Self {
        Self::new()
    }
}

/// vDSO clock data structure
#[repr(C)]
pub struct VdsoClock {
    /// Sequence counter for lockless reads
    pub seq: AtomicU32,
    /// Clock mode
    pub clock_mode: i32,
    /// Last cycle counter value
    pub cycle_last: AtomicU64,
    /// Clocksource mask
    pub mask: u64,
    /// Multiplier for cycle to nanoseconds conversion
    pub mult: u32,
    /// Shift for cycle to nanoseconds conversion
    pub shift: u32,
    /// Base time for each clock
    pub basetime: [VdsoTimestamp; VDSO_BASES],
    /// Unused
    pub _unused: u32,
}

impl VdsoClock {
    /// Create a new VdsoClock with default values.
    pub const fn new() -> Self {
        Self {
            seq: AtomicU32::new(0),
            clock_mode: VDSO_CLOCKMODE_TSC,
            cycle_last: AtomicU64::new(0),
            mask: u64::MAX,
            mult: 0,
            shift: 32,
            basetime: [VdsoTimestamp::new(); VDSO_BASES],
            _unused: 0,
        }
    }

    fn write_seqcount_begin(&self) {
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(1), Ordering::Release);
        core::sync::atomic::fence(Ordering::SeqCst);
    }

    fn write_seqcount_end(&self) {
        core::sync::atomic::fence(Ordering::SeqCst);
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(1), Ordering::Release);
    }

}

impl Default for VdsoClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Main vDSO data placed in `.data` (aligned to 4K)
#[repr(C)]
#[repr(align(4096))]
pub struct VdsoData {
    /// Clock data
    pub clocks: [VdsoClock; 2],
    /// Timezone minutes west of Greenwich
    pub tz_minuteswest: i32,
    /// Timezone DST time
    pub tz_dsttime: i32,
    /// High-resolution timer resolution in nanoseconds
    pub hrtimer_res: u32,
}

impl VdsoData {
    /// Create a new VdsoData with default values.
    pub const fn new() -> Self {
        Self {
            clocks: [VdsoClock::new(), VdsoClock::new()],
            tz_minuteswest: 0,
            tz_dsttime: 0,
            hrtimer_res: 1,
        }
    }

    /// Update vDSO clocks and basetimes.
    pub fn update(&mut self) {
        let cycle_now = current_ticks();
        let wall_ns = wall_time_nanos();
        let mono_ns = monotonic_time_nanos();

        let ticks_per_sec = nanos_to_ticks(NANOS_PER_SEC);
        let mult_shift = clocks_calc_mult_shift(ticks_per_sec, NANOS_PER_SEC, 10);

        for clk in &mut self.clocks {
            clk.write_seqcount_begin();

            let prev_cycle = clk.cycle_last.load(Ordering::Relaxed);
            let prev_basetime_ns = clk.basetime[1].sec.wrapping_mul(NANOS_PER_SEC)
                .wrapping_add(clk.basetime[1].nsec);

            match clk.clock_mode {
                VDSO_CLOCKMODE_TSC => {
                    if prev_cycle == 0 {
                        let (mult, shift) = mult_shift;
                        clk.mult = mult;
                        clk.shift = shift;
                        clk.cycle_last.store(0, Ordering::Relaxed);
                        clk.basetime[1].sec = 0;
                        clk.basetime[1].nsec = 0;
                    } else {
                        let (mult, shift) = mult_shift;
                        if !(mult == u32::MAX && shift == 0) {
                            clk.mult = mult;
                            clk.shift = shift;
                        } else {
                            let delta_cycles = (cycle_now.wrapping_sub(prev_cycle)) & clk.mask;
                            let delta_ns = mono_ns.saturating_sub(prev_basetime_ns);
                            if delta_cycles != 0 && delta_ns > 0 {
                                let (mult, shift) = clocks_calc_mult_shift(delta_cycles, delta_ns, 1);
                                clk.mult = mult;
                                clk.shift = shift;
                                clk.cycle_last.store(cycle_now, Ordering::Relaxed);
                                clk.basetime[1].sec = mono_ns / NANOS_PER_SEC;
                                clk.basetime[1].nsec = mono_ns % NANOS_PER_SEC;
                            }
                        }
                    }
                }

                VDSO_CLOCKMODE_PVCLOCK => {
                    // TODO: Implement PV clock (paravirt/pvclock) support.
                    clk.mult = 0;
                    clk.shift = 32;
                    clk.cycle_last.store(0, Ordering::Relaxed);
                    clk.basetime[1].sec = mono_ns / NANOS_PER_SEC;
                    clk.basetime[1].nsec = mono_ns % NANOS_PER_SEC;
                }
                VDSO_CLOCKMODE_NONE => {
                    // No cycle->ns conversion; store direct monotonic ns.
                    clk.mult = 0;
                    clk.cycle_last.store(0, Ordering::Relaxed);
                    clk.basetime[1].sec = mono_ns / NANOS_PER_SEC;
                    clk.basetime[1].nsec = mono_ns % NANOS_PER_SEC;
                }
                _ => {
                    // Unknown/unsupported clock mode; treat like NONE.
                    clk.mult = 0;
                    clk.cycle_last.store(0, Ordering::Relaxed);
                    clk.basetime[1].sec = mono_ns / NANOS_PER_SEC;
                    clk.basetime[1].nsec = mono_ns % NANOS_PER_SEC;
                }
            }

            // Update realtime and boottime entries.
            clk.basetime[0].sec = wall_ns / NANOS_PER_SEC;
            clk.basetime[0].nsec = wall_ns % NANOS_PER_SEC;
            clk.basetime[7].sec = clk.basetime[1].sec;
            clk.basetime[7].nsec = clk.basetime[1].nsec;

            if clk.seq.load(Ordering::Relaxed) < 10 {
                let cycle_val = clk.cycle_last.load(Ordering::Relaxed);
                trace!(
                    "vDSO update: seq={}, cycle_last={}, mono_ns={}, mult={}, shift={}",
                    clk.seq.load(Ordering::Relaxed),
                    cycle_val,
                    mono_ns,
                    clk.mult,
                    clk.shift
                );
            }

            clk.write_seqcount_end();
        }
    }
}

impl Default for VdsoData {
    fn default() -> Self {
        Self::new()
    }
}

/// Global vDSO data instance
#[unsafe(link_section = ".data")]
static mut VDSO_DATA: VdsoData = VdsoData::new();

/// Initialize vDSO data
pub fn init_vdso_data() {
    unsafe {
        let data_ptr = core::ptr::addr_of_mut!(VDSO_DATA);
        (*data_ptr).update();
        info!("vDSO data initialized at {:#x}", data_ptr as usize);
    }
}

/// Update vDSO data
pub fn update_vdso_data() {
    unsafe {
        let data_ptr = core::ptr::addr_of_mut!(VDSO_DATA);
        (*data_ptr).update();
    }
}

/// Get the physical address of vDSO data for mapping to userspace
pub fn vdso_data_paddr() -> usize {
    let data_ptr = core::ptr::addr_of!(VDSO_DATA) as usize;
    virt_to_phys(data_ptr.into()).into()
}


/// Load vDSO into the given user address space and update auxv accordingly.
pub fn load_vdso_data(auxv: &mut Vec<AuxEntry>, uspace: &mut AddrSpace) -> AxResult<(usize, Option<usize>)> {
    unsafe extern "C" {
        static vdso_start: usize;
        static vdso_end: usize;
    }
    let (vdso_kstart, vdso_kend) = unsafe {
        (
            &vdso_start as *const usize as usize,
            &vdso_end as *const usize as usize,
        )
    };


    const VDSO_USER_ADDR_BASE: usize = 0x7f00_0000;

    const VDSO_ASLR_PAGES: usize = 256;

    let rnd = || {
        let seed = (axhal::time::current_ticks() as u64) ^ (vdso_kstart as u64);
        let stack_addr = (&vdso_kstart as *const usize as u64).wrapping_shr(4);
        let data_addr = (core::ptr::addr_of!(VDSO_DATA) as usize as u64).wrapping_shr(4);
        let x = seed.wrapping_add(stack_addr).wrapping_add(data_addr).wrapping_add(0x9E3779B97F4A7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    };
    let page_off = (rnd() % (VDSO_ASLR_PAGES as u64)) as usize;
    let vdso_user_addr = VDSO_USER_ADDR_BASE + page_off * PAGE_SIZE_4K;
    info!("vdso_kstart: {vdso_kstart}, vdso_kend: {vdso_kend}",);

    if vdso_kend > vdso_kstart {
        let vdso_paddr = virt_to_phys(vdso_kstart.into());
        let vdso_size = (vdso_kend - vdso_kstart + PAGE_SIZE_4K - 1) & !(PAGE_SIZE_4K - 1);

        // Parse the embedded vDSO ELF and map each PT_LOAD segment with correct permissions.
        let vdso_len = vdso_kend - vdso_kstart;
        let vdso_bytes = unsafe { core::slice::from_raw_parts(vdso_kstart as *const u8, vdso_len) };

        match kernel_elf_parser::ELFHeadersBuilder::new(vdso_bytes).and_then(|b| {
            let range = b.ph_range();
            b.build(&vdso_bytes[range.start as usize..range.end as usize])
        }) {
            Ok(headers) => {
                info!("vDSO ELF parsed successfully, mapping segments");
                for ph in headers
                    .ph
                    .iter()
                    .filter(|ph| ph.get_type() == Ok(xmas_elf::program::Type::Load))
                {
                    let vaddr = ph.virtual_addr as usize;
                    let seg_pad = vaddr.align_offset_4k();
                    let seg_align_size =
                        (ph.mem_size as usize + seg_pad + PAGE_SIZE_4K - 1) & !(PAGE_SIZE_4K - 1);
                    let seg_user_start = vdso_user_addr + vaddr.align_down_4k();
                    let seg_paddr = vdso_paddr + vaddr.align_down_4k();

                    let flags = mapping_flags(ph.flags);
                    uspace
                        .map_linear(
                            seg_user_start.into(),
                            seg_paddr,
                            seg_align_size,
                            flags,
                        )
                        .map_err(|_| AxError::InvalidExecutable)?;
                }
            }
            Err(_) => {
                // Fallback: map the whole vdso region as RX if parsing fails.
                info!("vDSO ELF parsing failed, using fallback mapping");
                uspace
                    .map_linear(
                        vdso_user_addr.into(),
                        vdso_paddr,
                        vdso_size,
                        MappingFlags::READ | MappingFlags::EXECUTE | MappingFlags::USER,
                    )
                    .map_err(|_| AxError::InvalidExecutable)?;
            }
        }

        #[cfg(target_arch = "riscv64")]
        const VVAR_PAGES: usize = 2;
        #[cfg(target_arch = "loongarch64")]
        const VVAR_PAGES: usize = 44;
        #[cfg(target_arch = "x86_64")]
        const VVAR_PAGES: usize = 4;
        #[cfg(target_arch = "aarch64")]
        const VVAR_PAGES: usize = 5;

        let vvar_user_addr = vdso_user_addr - VVAR_PAGES * PAGE_SIZE_4K;
        let vvar_paddr = crate::vdso::vdso_data_paddr();

        uspace
            .map_linear(
                vvar_user_addr.into(),
                vvar_paddr.into(),
                VVAR_PAGES * PAGE_SIZE_4K,
                MappingFlags::READ | MappingFlags::USER,
            )
            .map_err(|_| AxError::InvalidExecutable)?;

        info!(
            "Mapped vvar pages at user {:#x}..{:#x} -> paddr {:#x}",
            vvar_user_addr,
            vvar_user_addr + VVAR_PAGES * PAGE_SIZE_4K,
            vvar_paddr,
        );

        const AT_SYSINFO_EHDR: usize = 33;
        let aux_pair: (usize, usize) = (AT_SYSINFO_EHDR, vdso_user_addr);
        let aux_entry: AuxEntry = unsafe { core::mem::transmute(aux_pair) };
        auxv.push(aux_entry);

    } else {
        warn!(
            "vDSO binary is missing or invalid: vdso_kstart={:#x}, vdso_kend={:#x}. vDSO will not be loaded and AT_SYSINFO_EHDR will not be set.",
            vdso_kstart, vdso_kend
        );
        return Err(AxError::InvalidExecutable);
    }
    return Ok((vdso_user_addr, None));
}

fn mapping_flags(flags: xmas_elf::program::Flags) -> MappingFlags {
    let mut mapping_flags = MappingFlags::USER;
    if flags.is_read() {
        mapping_flags |= MappingFlags::READ;
    }
    if flags.is_write() {
        mapping_flags |= MappingFlags::WRITE;
    }
    if flags.is_execute() {
        mapping_flags |= MappingFlags::EXECUTE;
    }
    mapping_flags
}
