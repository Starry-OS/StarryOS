//! StarryOS xtask — build & run tool
//!
//! Usage:
//!   cargo xtask build [--arch riscv64] [OPTIONS]
//!   cargo xtask run   [--arch riscv64] [OPTIONS]
//!   cargo xtask rootfs [--arch riscv64]
//!
//! This replicates the functionality of `make ARCH=<arch> run` from the
//! Makefile-based build system, using pure Rust for portability.

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

/// StarryOS multi-architecture build & run tool
#[derive(Parser)]
#[command(
    name = "xtask",
    about = "Build and run StarryOS on different architectures"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build the kernel for a given architecture
    Build(BuildArgs),
    /// Build and run the kernel in QEMU
    Run(BuildArgs),
    /// Download root filesystem image
    Rootfs {
        /// Target architecture: riscv64, aarch64, x86_64, loongarch64
        #[arg(long, default_value = "riscv64")]
        arch: String,
    },
}

#[derive(Parser, Clone)]
struct BuildArgs {
    /// Target architecture: riscv64, aarch64, x86_64, loongarch64
    #[arg(long, default_value = "riscv64")]
    arch: String,

    /// Logging level: off, error, warn, info, debug, trace
    #[arg(long, default_value = "warn")]
    log: String,

    /// Build mode: release, debug
    #[arg(long, default_value = "release")]
    mode: String,

    /// Memory size (e.g., 1G, 512M, 128M)
    #[arg(long, default_value = "1G")]
    mem: String,

    /// Number of CPUs (omit to use platform default)
    #[arg(long)]
    smp: Option<u32>,

    /// Disable DWARF debug info (enabled by default)
    #[arg(long)]
    no_dwarf: bool,

    /// Disable block device / virtio-blk (enabled by default)
    #[arg(long)]
    no_blk: bool,

    /// Disable network device / virtio-net (enabled by default)
    #[arg(long)]
    no_net: bool,

    /// Device bus type: pci, mmio
    #[arg(long, default_value = "pci")]
    bus: String,

    /// Disk image file (default: rootfs-<arch>.img)
    #[arg(long)]
    disk_img: Option<String>,

    /// IP address for the guest network
    #[arg(long, default_value = "10.0.2.15")]
    ip: String,

    /// Gateway address for the guest network
    #[arg(long, default_value = "10.0.2.2")]
    gw: String,
}

// ---------------------------------------------------------------------------
// Architecture / platform mapping
// ---------------------------------------------------------------------------

struct ArchInfo {
    target: &'static str,
    plat_package: &'static str,
    objcopy_arch: &'static str,
}

fn arch_info(arch: &str) -> ArchInfo {
    match arch {
        "riscv64" => ArchInfo {
            target: "riscv64gc-unknown-none-elf",
            plat_package: "axplat-riscv64-qemu-virt",
            objcopy_arch: "riscv64",
        },
        "aarch64" => ArchInfo {
            target: "aarch64-unknown-none-softfloat",
            plat_package: "axplat-aarch64-qemu-virt",
            objcopy_arch: "aarch64",
        },
        "x86_64" => ArchInfo {
            target: "x86_64-unknown-none",
            plat_package: "axplat-x86-pc",
            objcopy_arch: "x86_64",
        },
        "loongarch64" => ArchInfo {
            target: "loongarch64-unknown-none-softfloat",
            plat_package: "axplat-loongarch64-qemu-virt",
            objcopy_arch: "loongarch64",
        },
        _ => {
            eprintln!(
                "Error: unsupported architecture '{arch}'. \
                 Supported: riscv64, aarch64, x86_64, loongarch64"
            );
            process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Build output paths
// ---------------------------------------------------------------------------

struct BuildOutput {
    elf: PathBuf,
    bin: PathBuf,
    #[allow(dead_code)]
    platform: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Project root — the directory containing this crate's Cargo.toml.
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Parse a human-readable size string ("1G", "512M", "0x40000000b") into bytes.
fn parse_size(size_str: &str) -> u64 {
    let s = size_str.trim().to_lowercase();
    if s.starts_with("0x") {
        // Hexadecimal: must end with 'b'
        assert!(s.ends_with('b'), "Hex size must end with 'b'");
        u64::from_str_radix(&s[2..s.len() - 1], 16).expect("bad hex number")
    } else {
        let last = s.chars().last().unwrap();
        if last.is_ascii_digit() {
            // bare number → megabytes
            let n: f64 = s.parse().expect("bad number");
            (n * 1024.0 * 1024.0) as u64
        } else {
            let exp = match last {
                'b' => 0u32,
                'k' => 1,
                'm' => 2,
                'g' => 3,
                't' => 4,
                'p' => 5,
                'e' => 6,
                _ => panic!("Invalid size suffix '{last}'"),
            };
            let num: f64 = s[..s.len() - 1].parse().expect("bad number");
            (num * 1024f64.powi(exp as i32)) as u64
        }
    }
}

/// Run a command, capture stdout, return trimmed string.
fn cmd_stdout(program: &str, args: &[&str]) -> String {
    let out = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap_or_else(|e| {
            eprintln!("Error: failed to run '{program}': {e}");
            process::exit(1);
        });
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Run a command, inherit stdio, exit on failure.
fn run(program: &str, args: &[&str]) {
    println!("  {program} {}", args.join(" "));
    let st = Command::new(program)
        .args(args)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("Error: failed to run '{program}': {e}");
            process::exit(1);
        });
    if !st.success() {
        eprintln!("Error: '{program}' exited with {st}");
        process::exit(st.code().unwrap_or(1));
    }
}

/// Check whether a program is available on $PATH.
fn has_program(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_or(false, |s| s.success())
}

// ---------------------------------------------------------------------------
// Step 1 — ensure tools
// ---------------------------------------------------------------------------

fn ensure_tools() {
    if !has_program("cargo-axplat") {
        // `cargo axplat` dispatches to cargo-axplat
        let ok = Command::new("cargo")
            .args(["axplat", "--version"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_or(false, |s| s.success());
        if !ok {
            println!("Installing cargo-axplat …");
            run("cargo", &["install", "cargo-axplat"]);
        }
    }
    if !has_program("axconfig-gen") {
        println!("Installing axconfig-gen …");
        run("cargo", &["install", "axconfig-gen"]);
    }
    if !has_program("rust-objcopy") {
        println!("Installing cargo-binutils …");
        run("cargo", &["install", "cargo-binutils"]);
    }
}

// ---------------------------------------------------------------------------
// Step 2 — resolve platform
// ---------------------------------------------------------------------------

/// Returns (plat_config_path, plat_name).
fn resolve_platform(root: &Path, info: &ArchInfo) -> (String, String) {
    let plat_config = cmd_stdout(
        "cargo",
        &[
            "axplat",
            "info",
            "-C",
            root.to_str().unwrap(),
            "-c",
            info.plat_package,
        ],
    );
    if plat_config.is_empty() {
        eprintln!(
            "Error: could not resolve platform config for {}",
            info.plat_package
        );
        process::exit(1);
    }
    let raw = cmd_stdout("axconfig-gen", &[&plat_config, "-r", "platform"]);
    let plat_name = raw.trim_matches('"').to_string();
    (plat_config, plat_name)
}

// ---------------------------------------------------------------------------
// Step 3 — generate .axconfig.toml
// ---------------------------------------------------------------------------

/// Returns the effective SMP value.
fn generate_config(
    root: &Path,
    plat_config: &str,
    arch: &str,
    plat_name: &str,
    mem: &str,
    smp: Option<u32>,
) -> u32 {
    let out_config = root.join(".axconfig.toml");
    let defconfig = root.join("configs/defconfig.toml");
    let mem_bytes = parse_size(mem);

    let mut args: Vec<String> = vec![
        defconfig.to_str().unwrap().into(),
        plat_config.into(),
        "-w".into(),
        format!("arch=\"{arch}\""),
        "-w".into(),
        format!("platform=\"{plat_name}\""),
        "-w".into(),
        format!("plat.phys-memory-size={mem_bytes}"),
        "-o".into(),
        out_config.to_str().unwrap().into(),
    ];

    let smp_val = if let Some(s) = smp {
        args.extend(["-w".into(), format!("plat.cpu-num={s}")]);
        s
    } else {
        let raw = cmd_stdout("axconfig-gen", &[plat_config, "-r", "plat.cpu-num"]);
        raw.parse::<u32>().unwrap_or(1)
    };

    println!("  Generating config → .axconfig.toml");
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run("axconfig-gen", &refs);
    smp_val
}

// ---------------------------------------------------------------------------
// Step 4 — full build
// ---------------------------------------------------------------------------

fn do_full_build(root: &Path, args: &BuildArgs) -> BuildOutput {
    let info = arch_info(&args.arch);
    let dwarf = !args.no_dwarf;

    // 1. Tools
    ensure_tools();

    // 2. Platform
    let (plat_config, plat_name) = resolve_platform(root, &info);
    println!(
        "  Platform: {plat_name} ({})",
        info.plat_package
    );

    // 3. Config
    let smp = generate_config(root, &plat_config, &args.arch, &plat_name, &args.mem, args.smp);

    // 4. Feature list
    let mut features: Vec<&str> = vec!["qemu"];
    if dwarf {
        features.push("axfeat/dwarf");
    }
    if smp > 1 {
        features.push("smp");
    }
    let features_str = features.join(",");

    // 5. RUSTFLAGS (DWARF debug flags only; linker args come from build.rs)
    let mut rustflags = std::env::var("RUSTFLAGS").unwrap_or_default();
    if dwarf {
        if !rustflags.is_empty() {
            rustflags.push(' ');
        }
        rustflags.push_str("-C force-frame-pointers -C debuginfo=2 -C strip=none");
    }

    // 6. Mode
    let mode_dir = if args.mode == "release" {
        "release"
    } else {
        "debug"
    };

    // 7. cargo build
    let out_config = root.join(".axconfig.toml");
    println!(
        "  Building StarryOS for {} ({}) …",
        args.arch, info.target
    );

    let mut cmd = Command::new("cargo");
    cmd.current_dir(root)
        .arg("build")
        .args(["--target", info.target])
        .args(["--features", &features_str])
        .env("AX_ARCH", &args.arch)
        .env("AX_PLATFORM", &plat_name)
        .env("AX_MODE", &args.mode)
        .env("AX_LOG", &args.log)
        .env("AX_TARGET", info.target)
        .env("AX_IP", &args.ip)
        .env("AX_GW", &args.gw)
        .env("AX_CONFIG_PATH", out_config.to_str().unwrap());

    if dwarf {
        cmd.env("DWARF", "y");
    }
    if args.mode == "release" {
        cmd.arg("--release");
    }
    if !rustflags.is_empty() {
        cmd.env("RUSTFLAGS", &rustflags);
    }

    let st = cmd.status().expect("failed to execute cargo build");
    if !st.success() {
        eprintln!("Error: cargo build failed");
        process::exit(st.code().unwrap_or(1));
    }

    // 8. Copy ELF
    let elf_src = root
        .join("target")
        .join(info.target)
        .join(mode_dir)
        .join("starryos");
    let out_elf = root.join(format!("starryos_{plat_name}.elf"));
    let out_bin = root.join(format!("starryos_{plat_name}.bin"));

    std::fs::copy(&elf_src, &out_elf).unwrap_or_else(|e| {
        eprintln!(
            "Error: cp {} → {}: {e}",
            elf_src.display(),
            out_elf.display()
        );
        process::exit(1);
    });
    println!("  ELF → {}", out_elf.display());

    // 9. DWARF processing
    if dwarf {
        let script = root.join("scripts/make/dwarf.sh");
        if script.exists() {
            let objcopy_arg =
                format!("rust-objcopy --binary-architecture={}", info.objcopy_arch);
            println!("  Processing DWARF debug info …");
            let st = Command::new("bash")
                .args([
                    script.to_str().unwrap(),
                    out_elf.to_str().unwrap(),
                    &objcopy_arg,
                ])
                .current_dir(root)
                .status();
            if let Err(e) = st {
                eprintln!("Warning: DWARF processing failed: {e}");
            }
        }
    }

    // 10. objcopy → binary
    println!("  Creating binary image …");
    let st = Command::new("rust-objcopy")
        .args([
            &format!("--binary-architecture={}", info.objcopy_arch),
            out_elf.to_str().unwrap(),
            "--strip-all",
            "-O",
            "binary",
            out_bin.to_str().unwrap(),
        ])
        .status()
        .expect("rust-objcopy not found (cargo install cargo-binutils)");
    if !st.success() {
        eprintln!("Error: rust-objcopy failed");
        process::exit(st.code().unwrap_or(1));
    }

    if std::fs::metadata(&out_bin).map_or(true, |m| m.len() == 0) {
        eprintln!("Error: empty kernel image — check your build configuration");
        process::exit(1);
    }

    println!("  Build complete → {}", out_bin.display());

    BuildOutput {
        elf: out_elf,
        bin: out_bin,
        platform: plat_name,
    }
}

// ---------------------------------------------------------------------------
// Step 5 — QEMU
// ---------------------------------------------------------------------------

fn do_run_qemu(root: &Path, args: &BuildArgs, output: &BuildOutput) {
    let blk = !args.no_blk;
    let net = !args.no_net;

    let disk_img_name = args
        .disk_img
        .clone()
        .unwrap_or_else(|| format!("rootfs-{}.img", args.arch));
    let disk_path = root.join(&disk_img_name);

    if blk && !disk_path.exists() {
        eprintln!(
            "Error: disk image '{}' not found.\n\
             Run `cargo xtask rootfs --arch {}` to download it.",
            disk_path.display(),
            args.arch
        );
        process::exit(1);
    }

    let vdev = match args.bus.as_str() {
        "pci" => "pci",
        "mmio" => "device",
        other => {
            eprintln!("Error: bus must be 'pci' or 'mmio', got '{other}'");
            process::exit(1);
        }
    };

    // Read SMP from generated config
    let smp_str = if let Some(s) = args.smp {
        s.to_string()
    } else {
        let cfg = root.join(".axconfig.toml");
        let raw = cmd_stdout("axconfig-gen", &[cfg.to_str().unwrap(), "-r", "plat.cpu-num"]);
        raw.parse::<u32>().unwrap_or(1).to_string()
    };

    let qemu = format!("qemu-system-{}", args.arch);
    let mut qa: Vec<String> = vec![
        "-m".into(),
        args.mem.clone(),
        "-smp".into(),
        smp_str,
    ];

    // Architecture-specific
    match args.arch.as_str() {
        "riscv64" => qa.extend([
            "-machine".into(),
            "virt".into(),
            "-bios".into(),
            "default".into(),
            "-kernel".into(),
            output.bin.to_str().unwrap().into(),
        ]),
        "aarch64" => qa.extend([
            "-cpu".into(),
            "cortex-a72".into(),
            "-machine".into(),
            "virt".into(),
            "-kernel".into(),
            output.bin.to_str().unwrap().into(),
        ]),
        "x86_64" => qa.extend([
            "-machine".into(),
            "q35".into(),
            "-kernel".into(),
            output.elf.to_str().unwrap().into(),
        ]),
        "loongarch64" => qa.extend([
            "-machine".into(),
            "virt".into(),
            "-kernel".into(),
            output.bin.to_str().unwrap().into(),
        ]),
        _ => unreachable!(),
    }

    // Block device
    if blk {
        qa.extend([
            "-device".into(),
            format!("virtio-blk-{vdev},drive=disk0"),
            "-drive".into(),
            format!(
                "id=disk0,if=none,format=raw,file={}",
                disk_path.to_str().unwrap()
            ),
        ]);
    }

    // Network device
    if net {
        qa.extend([
            "-device".into(),
            format!("virtio-net-{vdev},netdev=net0"),
            "-netdev".into(),
            "user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555".into(),
        ]);
    }

    // No graphics
    qa.push("-nographic".into());

    println!("  Running: {qemu} {}", qa.join(" "));

    let st = Command::new(&qemu)
        .args(&qa)
        .current_dir(root)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("Error: failed to run {qemu}: {e}");
            process::exit(1);
        });

    if !st.success() {
        process::exit(st.code().unwrap_or(1));
    }
}

// ---------------------------------------------------------------------------
// Rootfs download
// ---------------------------------------------------------------------------

fn do_download_rootfs(root: &Path, arch: &str) {
    let rootfs_url = "https://github.com/Starry-OS/rootfs/releases/download/20250917";
    let img_name = format!("rootfs-{arch}.img");
    let img_path = root.join(&img_name);

    if img_path.exists() {
        println!("  Rootfs already exists: {}", img_path.display());
        return;
    }

    let xz_name = format!("{img_name}.xz");
    let xz_url = format!("{rootfs_url}/{xz_name}");
    let xz_path = root.join(&xz_name);

    println!("  Downloading {xz_url} …");
    run(
        "curl",
        &["-f", "-L", &xz_url, "-o", xz_path.to_str().unwrap()],
    );

    println!("  Decompressing …");
    run("xz", &["-d", xz_path.to_str().unwrap()]);

    println!("  Rootfs ready: {}", img_path.display());
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();
    let root = project_root();

    match cli.command {
        Cmd::Build(args) => {
            do_full_build(&root, &args);
        }
        Cmd::Run(args) => {
            let output = do_full_build(&root, &args);
            do_run_qemu(&root, &args, &output);
        }
        Cmd::Rootfs { arch } => {
            do_download_rootfs(&root, &arch);
        }
    }
}
