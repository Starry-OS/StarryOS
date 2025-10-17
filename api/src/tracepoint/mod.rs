mod event;
mod trace;
mod trace_pipe;

use alloc::{string::ToString, sync::Arc};

use axcpu::asm::flush_tlb;
use axhal::{paging::MappingFlags, percpu::this_cpu_id, time::monotonic_time_nanos};
use axtask::current;
use lazyinit::LazyInit;
use memory_addr::{PAGE_SIZE_4K, VirtAddr};
use starry_core::{
    task::AsThread,
    vfs::{DirMaker, DirMapping, SimpleDir, SimpleFile, SimpleFs},
};
pub use trace::{dynamic_create_cmdline, dynamic_create_trace};
pub use trace_pipe::{TraceCmdLineSizeFile, TracePipeFile};
use tracepoint::{KernelTraceOps, TraceEntryParser, TracePipeOps, TracingEventsManager};

use crate::{lock_api::KSpinNoPreempt, vfs::debug::DebugFsFile};

mod tests {
    use static_keys::{define_static_key_false_generic, static_branch_unlikely};

    use crate::tracepoint::KernelTraceAux;
    define_static_key_false_generic!(
        MY_STATIC_KEY,
        tracepoint::KernelCodeManipulator<KernelTraceAux>
    );
    #[inline(always)]
    fn foo() {
        ax_println!("Entering foo function");
        if static_branch_unlikely!(MY_STATIC_KEY) {
            ax_println!("A branch");
        } else {
            ax_println!("B branch");
        }
    }

    pub(super) fn static_keys_test() {
        foo();
        unsafe {
            MY_STATIC_KEY.enable();
        }
        foo();
    }
}

static TRACE_POINT_MANAGER: LazyInit<TracingEventsManager<KSpinNoPreempt<()>, KernelTraceAux>> =
    LazyInit::new();

pub fn tracepoint_manager() -> &'static TracingEventsManager<KSpinNoPreempt<()>, KernelTraceAux> {
    &TRACE_POINT_MANAGER
}

static TRACE_RAW_PIPE: KSpinNoPreempt<tracepoint::TracePipeRaw> =
    KSpinNoPreempt::new(tracepoint::TracePipeRaw::new(4096));

static TRACE_CMDLINE_CACHE: KSpinNoPreempt<tracepoint::TraceCmdLineCache> =
    KSpinNoPreempt::new(tracepoint::TraceCmdLineCache::new(128));

pub struct KernelTraceAux;

impl KernelTraceOps for KernelTraceAux {
    fn time_now() -> u64 {
        monotonic_time_nanos()
    }

    fn cpu_id() -> u32 {
        this_cpu_id() as _
    }

    fn current_pid() -> u32 {
        let curr = current();
        let proc_data = &curr.as_thread().proc_data;
        proc_data.proc.pid()
    }

    fn trace_pipe_push_raw_record(buf: &[u8]) {
        // log::debug!("trace_pipe_push_raw_record: {}", record.len());
        TRACE_RAW_PIPE.lock().push_event(buf.to_vec());
    }

    fn trace_cmdline_push(pid: u32) {
        let curr = current();
        let proc_data = &curr.as_thread().proc_data;
        let exe_path = proc_data.exe_path.read();
        let pname = exe_path
            .split(' ')
            .next()
            .unwrap_or("unknown")
            .split('/')
            .next_back()
            .unwrap_or("unknown");
        TRACE_CMDLINE_CACHE.lock().insert(pid, pname.to_string());
    }

    fn write_kernel_text(addr: *mut core::ffi::c_void, data: &[u8]) {
        let page_size = PAGE_SIZE_4K;

        let aligned_addr_val = (addr as usize) / page_size * page_size;
        let aligned_addr = aligned_addr_val as *mut core::ffi::c_void;
        let aligned_length = if (addr as usize) + data.len() - aligned_addr_val > page_size {
            page_size * 2
        } else {
            page_size
        };

        let kspace = axmm::kernel_aspace();
        let virt_addr = VirtAddr::from_usize(aligned_addr as usize);
        kspace
            .lock()
            .protect(
                virt_addr,
                aligned_length,
                MappingFlags::READ | MappingFlags::EXECUTE | MappingFlags::WRITE,
            )
            .expect("Failed to change page permissions");
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), addr as _, data.len());
            // Ensure the instruction cache is coherent with the modified code
            flush_tlb(Some(virt_addr));
        }
        kspace
            .lock()
            .protect(
                virt_addr,
                aligned_length,
                MappingFlags::READ | MappingFlags::EXECUTE,
            )
            .expect("Failed to restore page permissions");
    }
}

fn common_trace_pipe_read(trace_buf: &mut dyn TracePipeOps, buf: &mut [u8]) -> usize {
    let manager = tracepoint_manager();
    let tracepoint_map = manager.tracepoint_map();
    let trace_cmdline_cache = TRACE_CMDLINE_CACHE.lock();
    // read real trace data
    let mut copy_len = 0;
    let mut peek_flag = false;
    loop {
        if let Some(record) = trace_buf.peek() {
            let record_str = TraceEntryParser::parse::<KernelTraceAux, _>(
                &tracepoint_map,
                &trace_cmdline_cache,
                record,
            );
            if copy_len + record_str.len() > buf.len() {
                break; // Buffer is full
            }
            let len = record_str.len();
            buf[copy_len..copy_len + len].copy_from_slice(record_str.as_bytes());
            copy_len += len;
            peek_flag = true;
        }
        if peek_flag {
            trace_buf.pop(); // Remove the record after reading
            peek_flag = false;
        } else {
            break; // No more records to read
        }
    }
    copy_len
}

/// Initialize static keys for tracepoints.
pub fn tracepoint_init() {
    TRACE_POINT_MANAGER.call_once(|| {
        static_keys::global_init();
        tracepoint::global_init_events::<KSpinNoPreempt<()>, KernelTraceAux>()
            .expect("Failed to init tracepoint events")
    });
    // TODO: update text section permissions
    tests::static_keys_test();
}

/// Initialize events directory in debugfs
pub fn init_events(fs: Arc<SimpleFs>) -> DirMaker {
    let mut events_root = DirMapping::new();

    let events_manager = tracepoint_manager();
    // Register the global tracing events manager
    for subsystem_name in events_manager.subsystem_names() {
        let subsystem = events_manager.get_subsystem(&subsystem_name).unwrap();
        let mut subsystem_root = DirMapping::new();

        for event_name in subsystem.event_names() {
            let event_info = subsystem.get_event(&event_name).unwrap();

            let mut event_root = DirMapping::new();
            event_root.add(
                "enable",
                DebugFsFile::new_regular(
                    fs.clone(),
                    event::EventEnableFile::new(event_info.clone()),
                ),
            );
            event_root.add(
                "format",
                SimpleFile::new_regular(fs.clone(), {
                    let event_info = event_info.clone();
                    move || Ok(event_info.format_file().read())
                }),
            );
            event_root.add(
                "id",
                SimpleFile::new_regular(fs.clone(), {
                    let event_info = event_info.clone();
                    move || Ok(event_info.id_file().read())
                }),
            );

            subsystem_root.add(
                event_name,
                SimpleDir::new_maker(fs.clone(), Arc::new(event_root)),
            );
        }
        events_root.add(
            subsystem_name,
            SimpleDir::new_maker(fs.clone(), Arc::new(subsystem_root)),
        );
    }
    SimpleDir::new_maker(fs, Arc::new(events_root))
}
