use alloc::{
    borrow::Cow,
    boxed::Box,
    format,
    string::{String, ToString},
    sync::{Arc, Weak},
    vec,
    vec::Vec,
};
use core::{ffi::CStr, fmt::Write, iter};

use axalloc::{UsageKind, global_allocator};
use axfs_ng_vfs::{Filesystem, NodeType, VfsError, VfsResult};
use axhal::paging::MappingFlags;
use axtask::{AxTaskRef, WeakAxTaskRef, current};
use indoc::indoc;
use starry_core::{
    config::{USER_HEAP_BASE, USER_STACK_TOP},
    task::{AsThread, TaskStat, get_task, tasks},
    vfs::{
        DirMaker, DirMapping, NodeOpsMux, RwFile, SimpleDir, SimpleDirOps, SimpleFile,
        SimpleFileOperation, SimpleFs,
    },
};
use starry_process::Process;

use crate::{
    file::FD_TABLE,
    vfs::seq_file::{SeqFileNode, SeqIterator},
};

// Helper constant for unit conversion clarity
const KB: usize = 1024;
const PAGE_SIZE: usize = 0x1000;

pub fn meminfo_read() -> VfsResult<Vec<u8>> {
    let allocator = global_allocator();

    // Basic Pages Statistics
    // We access the page allocator to get the raw physical page counts.
    let used_pages = allocator.used_pages();
    let free_pages = allocator.available_pages();
    let total_pages = used_pages + free_pages;

    let total_kb = (total_pages * PAGE_SIZE) / KB;
    let free_kb = (free_pages * PAGE_SIZE) / KB;

    // Granular Usage Statistics (Snapshot)
    // We capture a snapshot of the usage tracker to avoid holding the lock for too long.
    let usages = allocator.usages();

    // Helper closure to convert bytes to KiB safely
    let to_kb = |kind| usages.get(kind) / KB;

    let heap_kb = to_kb(UsageKind::RustHeap);
    let cache_kb = to_kb(UsageKind::PageCache);
    let pg_tbl_kb = to_kb(UsageKind::PageTable);
    let user_kb = to_kb(UsageKind::VirtMem);
    let dma_kb = to_kb(UsageKind::Dma);

    // Derived Metrics
    // MemAvailable: An estimate of how much memory is available for starting new applications.
    // In Linux, this includes free memory + reclaimable caches.
    // For StarryOS v1, we assume PageCache is reclaimable.
    let available_kb = free_kb + cache_kb;

    // Fields set to 0 are placeholders for features not yet implemented in StarryOS.
    let content = format!(
        indoc! {"
        MemTotal:       {:8} kB
        MemFree:        {:8} kB
        MemAvailable:   {:8} kB
        Buffers:               0 kB
        Cached:         {:8} kB
        SwapCached:            0 kB
        Active:                0 kB
        Inactive:              0 kB
        SwapTotal:             0 kB
        SwapFree:              0 kB
        Dirty:                 0 kB
        Writeback:             0 kB
        AnonPages:      {:8} kB
        Mapped:         {:8} kB
        Shmem:                 0 kB
        Slab:           {:8} kB
        SReclaimable:          0 kB
        SUnreclaim:     {:8} kB
        PageTables:     {:8} kB
        NFS_Unstable:          0 kB
        Bounce:                0 kB
        CmaTotal:       {:8} kB
        HugePages_Total:       0
        HugePages_Free:        0
        Hugepagesize:       2048 kB
        DirectMap4k:    {:8} kB
        DirectMap2M:           0 kB
        "},
        total_kb,     // MemTotal
        free_kb,      // MemFree
        available_kb, // MemAvailable
        cache_kb,     // Cached
        user_kb,      // AnonPages (Userspace anonymous memory)
        user_kb,      // Mapped (Approximated as equal to AnonPages for now)
        heap_kb,      // Slab (Kernel heap usage)
        heap_kb,      // SUnreclaim (Kernel objects are mostly unreclaimable currently)
        pg_tbl_kb,    // PageTables
        dma_kb,       // CmaTotal
        total_kb      // DirectMap4k (Assuming 1:1 mapping for all memory)
    );

    Ok(content.into_bytes())
}

pub fn new_procfs() -> Filesystem {
    SimpleFs::new_with("proc".into(), 0x9fa0, builder)
}

struct ProcessTaskDir {
    fs: Arc<SimpleFs>,
    process: Weak<Process>,
}

impl SimpleDirOps for ProcessTaskDir {
    fn child_names<'a>(&'a self) -> Box<dyn Iterator<Item = Cow<'a, str>> + 'a> {
        let Some(process) = self.process.upgrade() else {
            return Box::new(iter::empty());
        };
        Box::new(
            process
                .threads()
                .into_iter()
                .map(|tid| tid.to_string().into()),
        )
    }

    fn lookup_child(&self, name: &str) -> VfsResult<NodeOpsMux> {
        let process = self.process.upgrade().ok_or(VfsError::NotFound)?;
        let tid = name.parse::<u32>().map_err(|_| VfsError::NotFound)?;
        let task = get_task(tid).map_err(|_| VfsError::NotFound)?;
        if task.as_thread().proc_data.proc.pid() != process.pid() {
            return Err(VfsError::NotFound);
        }

        Ok(NodeOpsMux::Dir(SimpleDir::new_maker(
            self.fs.clone(),
            Arc::new(ThreadDir {
                fs: self.fs.clone(),
                task: Arc::downgrade(&task),
            }),
        )))
    }

    fn is_cacheable(&self) -> bool {
        false
    }
}

#[rustfmt::skip]
fn task_status(task: &AxTaskRef) -> String {
    format!(
        "Tgid:\t{}\n\
        Pid:\t{}\n\
        Uid:\t0 0 0 0\n\
        Gid:\t0 0 0 0\n\
        Cpus_allowed:\t1\n\
        Cpus_allowed_list:\t0\n\
        Mems_allowed:\t1\n\
        Mems_allowed_list:\t0",
        task.as_thread().proc_data.proc.pid(),
        task.id().as_u64()
    )
}

/// The /proc/[pid]/fd directory
struct ThreadFdDir {
    fs: Arc<SimpleFs>,
    task: WeakAxTaskRef,
}

impl SimpleDirOps for ThreadFdDir {
    fn child_names<'a>(&'a self) -> Box<dyn Iterator<Item = Cow<'a, str>> + 'a> {
        let Some(task) = self.task.upgrade() else {
            return Box::new(iter::empty());
        };
        let ids = FD_TABLE
            .scope(&task.as_thread().proc_data.scope.read())
            .read()
            .ids()
            .map(|id| Cow::Owned(id.to_string()))
            .collect::<Vec<_>>();
        Box::new(ids.into_iter())
    }

    fn lookup_child(&self, name: &str) -> VfsResult<NodeOpsMux> {
        let fs = self.fs.clone();
        let task = self.task.upgrade().ok_or(VfsError::NotFound)?;
        let fd = name.parse::<u32>().map_err(|_| VfsError::NotFound)?;
        let path = FD_TABLE
            .scope(&task.as_thread().proc_data.scope.read())
            .read()
            .get(fd as _)
            .ok_or(VfsError::NotFound)?
            .inner
            .path()
            .into_owned();
        Ok(SimpleFile::new(fs, NodeType::Symlink, move || Ok(path.clone())).into())
    }

    fn is_cacheable(&self) -> bool {
        false
    }
}

/// VmaSnapshot use for lock-free maps format
struct VmaSnapshot {
    start: usize,
    end: usize,
    flags: MappingFlags,
    name: String,
}

/// iterator for travesing vma
struct ProcIterator {
    task: WeakAxTaskRef,
    index: usize,
}

impl ProcIterator {
    pub fn new(task: WeakAxTaskRef) -> Self {
        Self { task, index: 0 }
    }
}

impl SeqIterator for ProcIterator {
    type Item = VmaSnapshot;

    fn start(&mut self) -> Option<Self::Item> {
        self.index = 0;
        self.next()
    }

    fn next(&mut self) -> Option<Self::Item> {
        let task = self.task.upgrade()?;
        let proc = &task.as_thread().proc_data;
        let aspace = proc.aspace.lock();

        if let Some(area) = aspace.areas().nth(self.index) {
            self.index += 1;
            // Phase 1 Quick Win: Identify Stack and Heap by address
            let name = if area.end().as_usize() == USER_STACK_TOP {
                String::from("[stack]")
            } else if area.start().as_usize() == USER_HEAP_BASE {
                String::from("[heap]")
            } else {
                // TODO: Retrieve filename from FileBackend
                String::new()
            };
            Some(VmaSnapshot {
                start: area.start().as_usize(),
                end: area.end().as_usize(),
                flags: area.flags(),
                name,
            })
        } else {
            None
        }
    }

    fn show(&self, item: &Self::Item, buf: &mut String) -> core::fmt::Result {
        write!(buf, "{:012x}-{:012x} ", item.start, item.end)?;

        let r = if item.flags.contains(MappingFlags::READ) {
            'r'
        } else {
            '-'
        };
        let w = if item.flags.contains(MappingFlags::WRITE) {
            'w'
        } else {
            '-'
        };
        let x = if item.flags.contains(MappingFlags::EXECUTE) {
            'x'
        } else {
            '-'
        };

        // 'p' = Private (COW), 's' = Shared.
        // Currently hardcoded to 'p' as StarryOS defaults to private mappings.
        let s = 'p';
        write!(buf, "{}{}{}{} ", r, w, x, s)?;

        // Offset, Dev, Inode
        // Currently hardcoded to 0.
        // TODO: Retrieve actual offset/inode from Backend in the future.
        // Format: Offset(8 chars) Dev(Major:Minor) Inode
        write!(buf, "{:08x} 00:00 0", 0)?;

        // Pathname
        // Linux style: pad with spaces if a name exists, otherwise just newline.
        if !item.name.is_empty() {
            writeln!(buf, "                          {}", item.name)?;
        } else {
            writeln!(buf)?;
        }

        Ok(())
    }
}

/// The /proc/[pid] directory
struct ThreadDir {
    fs: Arc<SimpleFs>,
    task: WeakAxTaskRef,
}

impl SimpleDirOps for ThreadDir {
    fn child_names<'a>(&'a self) -> Box<dyn Iterator<Item = Cow<'a, str>> + 'a> {
        Box::new(
            [
                "stat",
                "status",
                "oom_score_adj",
                "task",
                "maps",
                "mounts",
                "cmdline",
                "comm",
                "exe",
                "fd",
            ]
            .into_iter()
            .map(Cow::Borrowed),
        )
    }

    fn lookup_child(&self, name: &str) -> VfsResult<NodeOpsMux> {
        let fs = self.fs.clone();
        let task = self.task.upgrade().ok_or(VfsError::NotFound)?;
        Ok(match name {
            "stat" => SimpleFile::new_regular(fs, move || {
                Ok(format!("{}", TaskStat::from_thread(&task)?).into_bytes())
            })
            .into(),
            "status" => SimpleFile::new_regular(fs, move || Ok(task_status(&task))).into(),
            "oom_score_adj" => SimpleFile::new_regular(
                fs,
                RwFile::new(move |req| match req {
                    SimpleFileOperation::Read => Ok(Some(
                        task.as_thread().oom_score_adj().to_string().into_bytes(),
                    )),
                    SimpleFileOperation::Write(data) => {
                        if !data.is_empty() {
                            let value = str::from_utf8(data)
                                .ok()
                                .and_then(|it| it.parse::<i32>().ok())
                                .ok_or(VfsError::InvalidInput)?;
                            task.as_thread().set_oom_score_adj(value);
                        }
                        Ok(None)
                    }
                }),
            )
            .into(),
            "task" => SimpleDir::new_maker(
                fs.clone(),
                Arc::new(ProcessTaskDir {
                    fs,
                    process: Arc::downgrade(&task.as_thread().proc_data.proc),
                }),
            )
            .into(),
            "maps" => {
                let task_ref = Arc::downgrade(&task);
                let iter = ProcIterator::new(task_ref);

                // Construct a fake Inode based on PID + Magic
                let inode = (task.id().as_u64() << 16) | 0x100;

                let node = SeqFileNode::new(iter, fs, inode);

                node.into()
            }
            "mounts" => SimpleFile::new_regular(fs, move || {
                Ok("proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0\n")
            })
            .into(),
            "cmdline" => SimpleFile::new_regular(fs, move || {
                let cmdline = task.as_thread().proc_data.cmdline.read();
                let mut buf = Vec::new();
                for arg in cmdline.iter() {
                    buf.extend_from_slice(arg.as_bytes());
                    buf.push(0);
                }
                Ok(buf)
            })
            .into(),
            "comm" => SimpleFile::new_regular(
                fs,
                RwFile::new(move |req| match req {
                    SimpleFileOperation::Read => {
                        let mut bytes = vec![0; 16];
                        let name = task.name();
                        let copy_len = name.len().min(15);
                        bytes[..copy_len].copy_from_slice(&name.as_bytes()[..copy_len]);
                        bytes[copy_len] = b'\n';
                        Ok(Some(bytes))
                    }
                    SimpleFileOperation::Write(data) => {
                        if !data.is_empty() {
                            let mut input = [0; 16];
                            let copy_len = data.len().min(15);
                            input[..copy_len].copy_from_slice(&data[..copy_len]);
                            task.set_name(
                                CStr::from_bytes_until_nul(&input)
                                    .map_err(|_| VfsError::InvalidInput)?
                                    .to_str()
                                    .map_err(|_| VfsError::InvalidInput)?,
                            );
                        }
                        Ok(None)
                    }
                }),
            )
            .into(),
            "exe" => SimpleFile::new(fs, NodeType::Symlink, move || {
                Ok(task.as_thread().proc_data.exe_path.read().clone())
            })
            .into(),
            "fd" => SimpleDir::new_maker(
                fs.clone(),
                Arc::new(ThreadFdDir {
                    fs,
                    task: Arc::downgrade(&task),
                }),
            )
            .into(),
            _ => return Err(VfsError::NotFound),
        })
    }

    fn is_cacheable(&self) -> bool {
        false
    }
}

/// Handles /proc/[pid] & /proc/self
struct ProcFsHandler(Arc<SimpleFs>);

impl SimpleDirOps for ProcFsHandler {
    fn child_names<'a>(&'a self) -> Box<dyn Iterator<Item = Cow<'a, str>> + 'a> {
        Box::new(
            tasks()
                .into_iter()
                .map(|task| task.id().as_u64().to_string().into())
                .chain([Cow::Borrowed("self")]),
        )
    }

    fn lookup_child(&self, name: &str) -> VfsResult<NodeOpsMux> {
        let task = if name == "self" {
            current().clone()
        } else {
            let tid = name.parse::<u32>().map_err(|_| VfsError::NotFound)?;
            get_task(tid).map_err(|_| VfsError::NotFound)?
        };
        let node = NodeOpsMux::Dir(SimpleDir::new_maker(
            self.0.clone(),
            Arc::new(ThreadDir {
                fs: self.0.clone(),
                task: Arc::downgrade(&task),
            }),
        ));
        Ok(node)
    }

    fn is_cacheable(&self) -> bool {
        false
    }
}

fn builder(fs: Arc<SimpleFs>) -> DirMaker {
    let mut root = DirMapping::new();
    root.add(
        "mounts",
        SimpleFile::new_regular(fs.clone(), || {
            Ok("proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0\n")
        }),
    );
    root.add(
        "meminfo",
        SimpleFile::new_regular(fs.clone(), || meminfo_read()),
    );
    root.add(
        "instret",
        SimpleFile::new_regular(fs.clone(), || {
            #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
            {
                Ok(format!("{}\n", riscv::register::instret::read64()))
            }
            #[cfg(not(any(target_arch = "riscv32", target_arch = "riscv64")))]
            {
                Ok("0\n".to_string())
            }
        }),
    );
    root.add(
        "interrupts",
        SimpleFile::new_regular(fs.clone(), || Ok(format!("0: {}", crate::time::irq_cnt()))),
    );

    root.add("sys", {
        let mut sys = DirMapping::new();

        sys.add("kernel", {
            let mut kernel = DirMapping::new();

            kernel.add(
                "pid_max",
                SimpleFile::new_regular(fs.clone(), || Ok("32768\n")),
            );

            SimpleDir::new_maker(fs.clone(), Arc::new(kernel))
        });

        SimpleDir::new_maker(fs.clone(), Arc::new(sys))
    });

    let proc_dir = ProcFsHandler(fs.clone());
    SimpleDir::new_maker(fs, Arc::new(proc_dir.chain(root)))
}
