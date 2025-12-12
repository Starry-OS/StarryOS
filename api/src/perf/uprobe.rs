use axfs_ng::{CachedFile, FS_CONTEXT};
use kbpf_basic::perf::{PerfProbeArgs, PerfProbeConfig};
use kprobe::ProbeBuilder;
use starry_core::task::{AsThread, get_task};

use crate::{
    kprobe::KprobeAuxiliary,
    perf::kprobe::{ProbePerfEvent, ProbeTy},
};

fn perf_probe_arg_to_uprobe_builder(args: &PerfProbeArgs) -> ProbeBuilder<KprobeAuxiliary> {
    let elf = &args.name;
    let offset = args.offset as usize;
    let pid = args.pid;

    axlog::error!(
        "perf_probe->uprobe pid: [{}], ELF: {}, offset: {:#x}",
        pid,
        elf,
        offset
    );

    if pid == -1 {
        // pid == -1 means for all processes(dyn lib, such as libc.so), which is not
        // supported for uprobe
        let loc = FS_CONTEXT
            .lock()
            .resolve(elf)
            .expect("Failed to resolve ELF(dyn lib)");
        let _lib = CachedFile::get_or_create(loc);
        panic!("uprobe for all processes is not supported");
    }

    let task = get_task(pid as _).expect("Failed to get task for uprobe");
    let mm = task.as_thread().proc_data.aspace.lock();
    let memset = mm.memoryset();
    let mut virt_base = None;

    for vma in memset.iter() {
        let backend = vma.backend();
        let loc = backend.location();
        if &loc == elf {
            // found the target ELF
            virt_base = Some(vma.start());
            break;
        }
    }

    drop(mm);
    assert!(
        virt_base.is_some(),
        "Failed to find mapped ELF {} in target process",
        elf
    );
    let virt_base = virt_base.unwrap();
    let virt_addr = offset + virt_base.as_usize();

    axlog::error!(
        "Found mapped ELF {} at virtual address: {:#x}, offset's virtual address: {:#x}",
        elf,
        virt_base.as_usize(),
        virt_addr
    );

    let builder = ProbeBuilder::new()
        .with_symbol(elf.clone())
        .with_symbol_addr(virt_addr)
        .with_offset(0)
        .with_user_mode(pid);
    builder
}

pub fn perf_event_open_uprobe(args: PerfProbeArgs) -> ProbePerfEvent {
    let elf = &args.name;
    axlog::trace!("create uprobe for file: {elf}");
    let probe = match args.config {
        PerfProbeConfig::Raw(val) => {
            if val == 0 {
                // uprobe
                let builder = perf_probe_arg_to_uprobe_builder(&args);
                let uprobe = crate::uprobe::register_uprobe(builder);
                ProbeTy::Uprobe(uprobe)
            } else if val == 1 {
                panic!("unsupported config for uretprobe");
            } else {
                panic!("unsupported config {} for uprobe", val);
            }
        }
        _ => {
            panic!("unsupported config {:?} for uprobe", args.config);
        }
    };

    axlog::trace!("create uprobe ok");
    ProbePerfEvent::new(args, probe)
}
