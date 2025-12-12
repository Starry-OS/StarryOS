# eBPF for Starry

## 总览
- [x] kprobes/kretprobes 支持
- [x] tracepoint/rawtracepoint 支持
- [x] uprobes 支持
- [x] eBPF 支持
- [ ] uretprobes 支持

本文档介绍 StarryOS 在内核态对 kprobe/kretprobe、tracepoint/rawtracepoint 以及与 eBPF 的集成实现与使用方法。内容涵盖工作原理、主要模块、使用方式、支持的典型用例与注意事项。

**适用对象**：需要在 StarryOS 中进行内核函数/事件动态观测、性能分析、故障定位、以及编写 eBPF 程序的开发者。

## 整体架构与核心概念

- **kprobe/kretprobe**：在指定的内核符号或地址处插桩，函数入口为 kprobe，函数返回为 kretprobe，用于捕获调用现场、参数与返回值。
- **tracepoint/rawtracepoint**：预定义的内核事件插桩点。`tracepoint` 提供结构化事件与上下文，`rawtracepoint` 提供原始、更贴近底层的事件接口。
- **uprobes/uretprobes**：在用户态程序的函数入口/返回处插桩，用于观测用户态行为（Starry 已支持 uprobes；uretprobes仍在规划中）。
- **eBPF**：安全可验证的字节码，运行在内核虚拟机中，结合上述插桩点对事件进行处理与过滤。StarryOS 提供 map、prog 管理以及与 perf/trace 子系统的桥接。

核心模块分布（主要路径）：
- `api/src/kprobe/`：kprobe/kretprobe 的用户接口与测试。
- `api/src/perf/`：perf 事件桥接层，含 `kprobe.rs`、`tracepoint.rs`、`raw_tracepoint.rs`、`uprobe.rs` 等。
- `api/src/tracepoint/`：tracepoint 目录与 debugfs 输出（如 `trace_pipe` 与 `events`）。
- `api/src/bpf/`：eBPF 程序与 map 管理（`map.rs`、`prog/mod.rs` 等）。
- `core/src/probe_aux.rs`：kprobe/uprobes 等辅助能力（地址写权限调整、用户/内核可执行页分配、retprobe 实例保存）。
- `core/src/lock_api.rs`：`KSpinNoPreempt` 基于 `lock_api` 的自旋锁封装，保障多核并发下的安全访问。

## 工作原理与实现细节

### kprobe/kretprobe

- **入口插桩（kprobe）**：在目标函数入口处改写指令或放置断点，进入探针处理路径；Starry 会解析符号或地址，并在命中时收集寄存器、参数等上下文，驱动到 eBPF 程序或回调。
- **返回插桩（kretprobe）**：在函数返回路径设置 `retprobe` 实例，函数返回时触发，收集返回值与延迟信息；`core/src/probe_aux.rs` 维护 `RetprobeInstance` 列表：
  - 当前任务存在时，实例链挂在任务结构中；
  - 无当前任务时，使用静态 `INSTANCE: KSpinNoPreempt<Vec<RetprobeInstance>>` 保存与弹出。
- **并发与锁**：使用 `KSpinNoPreempt` 保证在不可抢占环境中安全访问实例队列，`lock_api::RawMutex` 封装了非抢占自旋锁的 `lock/try_lock/unlock/is_locked` 行为。
- **代码修改与写权限**：当需要写入指令或修补文本段：
  - 用户态地址：`set_writeable_for_address_user` 会临时将页权限设为可写，触发 COW，执行修改后恢复原权限并刷新 TLB。
  - 内核态地址：`set_writeable_for_address_kernel` 使用 `kernel_aspace().protect(...)` 临时放开写权限，写入后恢复 `READ|EXECUTE`，并刷新 TLB。

### tracepoint/rawtracepoint

- **tracepoint**：通过 `api/src/tracepoint` 中的目录构造与文件实现，暴露 `events/` 配置与 `trace_pipe` 实时输出；`debug.rs` 中的 `new_debugfs()` 构建了 `debugfs` 文件系统，`tracing_dir` 下挂载：
  - `saved_cmdlines_size` 与 `saved_cmdlines`：展示/动态生成命令行缓存；
  - `trace_pipe`：类似 Linux 的实时事件流读取；
  - `trace`：当前跟踪信息的快照或配置文件；
  - `events`：事件目录与开关管理。
- **rawtracepoint**：以更原始形式接入底层事件，常用于性能敏感或需要最小开销的场景；在 `api/src/perf/raw_tracepoint.rs` 中与 perf 子系统集成。

### 与 eBPF 的桥接

- **程序加载与校验**：`api/src/bpf/prog` 负责 eBPF 程序的加载、类型匹配与附着（如 `kprobe`、`kretprobe`、`tracepoint` 等 program type）。
- **map 管理**：`api/src/bpf/map.rs` 提供 map 创建、查找、更新与删除，支持常见的哈希、数组、perf 事件缓冲等。
- **事件路径**：事件触发 → 进入 kprobe/tracepoint 回调 → 传递上下文到 eBPF → eBPF 对数据过滤/聚合 → 写入 map 或推送 perf buffer → 用户态消费。

### 可执行页分配与回收

在需要生成跳板或补丁代码时：
- **内核页**：`alloc_kernel_exec_memory` 分配 4K 可执行页并 `protect` 成 `READ|EXECUTE`，使用前先以可写方式修改，完成后恢复只读+执行；`free_kernel_exec_memory` 回收并恢复页权限。
- **用户页**：`alloc_user_exec_memory` 在指定进程地址空间寻找空闲区域，映射为可写，执行初始化动作后以 `READ|EXECUTE|USER` 挂入页表；`free_user_exec_memory` 解除映射并释放物理页。

## 使用方法

以下给出典型工作流与示例，具体 API 请参考 `api/src/perf/` 与 `api/src/kprobe/`。

### 在内核中启用 kprobe/kretprobe 并附着 eBPF 程序

1) 确定目标函数符号或地址（例如 `sysno`）。
2) 创建并注册 kprobe：
   - 指定入口探针，对函数入口事件进行采样；
   - 若需要返回值，则注册 kretprobe 并在返回路径收集结果。
3) 编写 eBPF 程序（program type = kprobe/kretprobe），在 `probe_ctx` 中读取参数/寄存器与返回值，写入 map缓冲。
4) 在用户态通过读取map的数据获得统计结果。

### 使用 tracepoint/rawtracepoint

1) 在 `events` 目录中启用目标事件（或使用内核 API 直接附着）。
2) 为该事件类型编写 eBPF 程序（program type = tracepoint/rawtracepoint），使用事件上下文进行过滤与聚合。
3) 在用户态通过 perf buffer 或 `trace_pipe` 消费数据。

### 用户态 uprobes 附着 eBPF 程序

1) 为目标进程与函数设置 uprobes。
2) 在 `probe_aux.rs` 的用户态辅助函数协助下临时修改文本段与分配执行页。
3) 编写 eBPF 程序（program type = uprobe），获取参数、栈信息等。
4) 用户态读取 map 或 perf buffer 获取结果。

## 支持的典型用例

- **性能分析**：函数调用频次统计、延迟分布（kprobe/kretprobe + perf buffer）。
- **网络事件观测**：TCP/UDP 收发路径的 tracepoint/rawtracepoint 事件抽样。
- **系统调用跟踪**：syscall 入口/返回（kprobe/kretprobe 或 tracepoint），结合 eBPF 过滤用户/进程维度。
- **用户态热点定位**：对关键用户态函数附着 uprobes，统计热点与异常返回。
- **实时事件流**：通过 `trace_pipe` 输出，便于在线诊断与演示。


## 快速上手（示例流程）
以下示例为概念性流程，便于对齐 StarryOS 的使用方式：


### musl 目录下的示例程序

以下示例位于 `user/musl/`，均包含用户态驱动程序与对应的 eBPF 子包（`*-ebpf`）以及公共库（`*-common`）。按各自 README 使用 `cargo run --release` 即可（需要合适的 musl 目标与工具链，见`musl/Makefile`）。

- `user/musl/async_test/`：异步/IO 相关观测示例（用于基础环境与运行验证）。
- `user/musl/kret/`：kretprobe 示例，包含：
  - `kret/`（用户态驱动）
  - `kret-ebpf/`（eBPF 程序，类型为 kretprobe）
  - `kret-common/`（公共类型/常量）
- `user/musl/mytrace/`：tracepoint 示例，包含：
  - `mytrace/`（用户态驱动）
  - `mytrace-ebpf/`（eBPF 程序，类型为 tracepoint）
  - `mytrace-common/`（公共类型/常量）
- `user/musl/rawtp/`：rawtracepoint 示例，包含：
  - `rawtp/`（用户态驱动）
  - `rawtp-ebpf/`（eBPF 程序，类型为 rawtracepoint）
  - `rawtp-common/`（公共类型/常量）
- `user/musl/syscall_ebpf/`：系统调用 eBPF 示例，包含：
  - `syscall_ebpf/`（用户态驱动）
  - `syscall_ebpf-ebpf/`（eBPF 程序，类型为 kprobe）
  - `syscall_ebpf-common/`（公共类型/常量）
- `user/musl/upb/` 与 `user/musl/upb2/`：uprobes 示例，包含：
  - `upb/` 或 `upb2/`（用户态驱动）
  - `upb-ebpf/`（eBPF 程序，类型为 uprobe）
  - `upb-common/`（公共类型/常量）

示例通用运行方式（以 syscall_ebpf 为例）：

```sh
starry:~# /musl/syscall_ebpf
```

### tracepoint 的文件系统接口使用方法

使用方法:

- 使用`define_event_trace`定义跟踪点名称和回调函数原型，以及默认的打印方法
- 用户态在`/sys/kernel/debug/tracing/events/`进入对应子系统下查看存在的跟踪点
- `echo 1 > enable`开启
- `echo 0 > enable`关闭
- 使用`filter`文件设置过滤条件
- 使用`format`文件查看跟踪点的格式
- 查看`/sys/kernel/debug/tracing/trace`获取输出
- 也可以查看`/sys/kernel/debug/tracing/trace_pipe`获取输出

已`sys_mkdirat`的tp跟踪点为例：

```
starry:/sys/kernel/debug/tracing/events/syscalls/sys_mkdirat# ls -l
total 0
-rw-rw-rw-    1 root     root             0 Jan  1  1970 enable
-rw-rw-rw-    1 root     root             0 Jan  1  1970 format
-rw-rw-rw-    1 root     root             0 Jan  1  1970 id
```

1. 通过`echo 1 > enable` 使能该tp
2. 在/tmp目录下创建一些文件
3. `cat /sys/kernel/debug/tracing/trace`

```
starry:/# cat /sys/kernel/debug/tracing/trace
# tracer: nop
#
# entries-in-buffer/entries-written: 3/3   #P:32
#
#
#                                _-----=> irqs-off/BH-disabled
#                               / _----=> need-resched
#                              | / _---=> hardirq/softirq
#                              || / _--=> preempt-depth
#                              ||| / _-=> migrate-disable
#                              |||| /     delay
#           TASK-PID     CPU#  |||||  TIMESTAMP  FUNCTION
#              | |         |   |||||     |         |
         busybox-17      [000] .....   231.451497: sys_mkdirat(mkdir at /tmp/d1 with mode NodePermission(OWNER_READ | OWNER_WRITE | OWNER_EXEC | GROUP_READ | GROUP_EXEC | OTHER_READ | OTHER_EXEC))
         busybox-18      [000] .....   231.451848: sys_mkdirat(mkdir at /tmp/d2 with mode NodePermission(OWNER_READ | OWNER_WRITE | OWNER_EXEC | GROUP_READ | GROUP_EXEC | OTHER_READ | OTHER_EXEC))
         busybox-19      [000] .....   231.451878: sys_mkdirat(mkdir at /tmp/d3 with mode NodePermission(OWNER_READ | OWNER_WRITE | OWNER_EXEC | GROUP_READ | GROUP_EXEC | OTHER_READ | OTHER_EXEC))
```

4. `echo 0 > enable` 关闭tp
5. `/sys/kernel/debug/tracing/events/syscalls/sys_mkdirat/filter` 可以设置过滤条件


## 注意事项
在编译musl工具链的用户态程序时，如果是动态链接，app会依赖`libgcc_s.so.1`动态库，而musl工具链默认并不包含该库。需要安装该库。在StarryOS中，可以通过以下命令安装：

```sh
apk add libgcc
```

在loongarch架构下，程序依赖的加载器路径位于`/lib64/ld-musl-loongarch-lp64d.so.1`，而不是默认的`/lib/ld-musl-loongarch64.so.1`。可以通过创建符号链接来解决这个问题
```sh
ln -s /lib/ld-musl-loongarch64.so.1 /lib64/ld-musl-loongarch-lp64d.so.1 
```

在aarch64架构下, 使用musl工具链不像其它架构一样会默认进行动态链接，而是默认进行静态链接。如果需要动态链接，需要指定 `rustflags = ["-C", "target-feature=-crt-static"]`。

## 参考与源码入口

- `core/src/probe_aux.rs`：地址权限调整、可执行页分配、retprobe 实例管理。
- `core/src/lock_api.rs`：不可抢占自旋锁封装，保证探针路径并发安全。
- `api/src/kprobe/`：kprobe/kretprobe 用户接口与测试。
- `api/src/perf/`：perf 桥接，含 kprobe/tracepoint/rawtracepoint/uprobes。
- `api/src/tracepoint/` 与 `api/src/vfs/debug.rs`：debugfs 构建、`trace_pipe`、`events`。
- `api/src/bpf/`：eBPF map 与 prog 管理。
- 开发文档：https://github.com/orgs/Starry-OS/discussions/4