use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::{self, Command};

/// StarryOS multi-architecture build & test tool
#[derive(Parser)]
#[command(
    name = "xtask",
    about = "Build, run and test StarryOS on different architectures"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Download rootfs image for the given architecture
    Rootfs {
        /// Target architecture: riscv64, aarch64, x86_64, loongarch64
        #[arg(long, default_value = "riscv64")]
        arch: String,
    },
    /// Build the kernel for the given architecture
    Build {
        /// Target architecture: riscv64, aarch64, x86_64, loongarch64
        #[arg(long, default_value = "riscv64")]
        arch: String,
    },
    /// Build and run the kernel in QEMU (interactive)
    Run {
        /// Target architecture: riscv64, aarch64, x86_64, loongarch64
        #[arg(long, default_value = "riscv64")]
        arch: String,
    },
    /// Run automated boot test via ci-test.py (checks BusyBox shell prompt)
    Test {
        /// Target architecture: riscv64, aarch64, x86_64, loongarch64
        #[arg(long, default_value = "riscv64")]
        arch: String,
    },
}

/// Returns true when running inside WSL / WSL2.
fn is_wsl() -> bool {
    std::fs::read_to_string("/proc/version")
        .map(|v| v.to_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

/// Locate the project root (directory containing the workspace Cargo.toml).
fn project_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points to the root package (StarryOS/), which is
    // where the Makefile lives.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Build a `make` Command pre-loaded with common arguments.
fn make_cmd(root: &Path, arch: &str) -> Command {
    let mut cmd = Command::new("make");
    cmd.current_dir(root)
        .arg(format!("ARCH={arch}"));
    cmd
}

/// Execute a Command and exit with its exit code on failure.
fn run(mut cmd: Command) {
    let status = cmd
        .status()
        .unwrap_or_else(|e| {
            eprintln!("xtask: failed to spawn process: {e}");
            process::exit(1);
        });
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
}

fn do_rootfs(root: &Path, arch: &str) {
    println!("==> make ARCH={arch} rootfs");
    let mut cmd = make_cmd(root, arch);
    cmd.arg("rootfs");
    run(cmd);
}

fn do_build(root: &Path, arch: &str) {
    println!("==> make ARCH={arch} build");
    let mut cmd = make_cmd(root, arch);
    cmd.arg("build");
    run(cmd);
}

fn do_run(root: &Path, arch: &str) {
    let mut cmd = make_cmd(root, arch);
    cmd.arg("run");
    if is_wsl() {
        cmd.arg("ACCEL=n");
    }
    println!("==> make ARCH={arch} run{}", if is_wsl() { " ACCEL=n" } else { "" });
    run(cmd);
}

fn do_test(root: &Path, arch: &str) {
    let script = root.join("scripts").join("ci-test.py");
    if !script.exists() {
        eprintln!("xtask: ci-test.py not found at {}", script.display());
        process::exit(1);
    }
    println!("==> python3 scripts/ci-test.py {arch}");
    let mut cmd = Command::new("python3");
    cmd.current_dir(root)
        .arg(&script)
        .arg(arch);
    run(cmd);
}

fn main() {
    let cli = Cli::parse();
    let root = project_root();

    validate_arch(match &cli.command {
        Cmd::Rootfs { arch } | Cmd::Build { arch } | Cmd::Run { arch } | Cmd::Test { arch } => {
            arch
        }
    });

    match &cli.command {
        Cmd::Rootfs { arch } => do_rootfs(&root, arch),
        Cmd::Build { arch } => do_build(&root, arch),
        Cmd::Run { arch } => do_run(&root, arch),
        Cmd::Test { arch } => do_test(&root, arch),
    }
}

fn validate_arch(arch: &str) {
    const SUPPORTED: &[&str] = &["riscv64", "aarch64", "x86_64", "loongarch64"];
    if !SUPPORTED.contains(&arch) {
        eprintln!(
            "xtask: unsupported architecture '{arch}'. Supported: {}",
            SUPPORTED.join(", ")
        );
        process::exit(1);
    }
}
