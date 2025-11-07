use anyhow::{Context as _, anyhow};
use aya_build::Toolchain;

fn main() -> anyhow::Result<()> {
    let cargo_metadata::Metadata { packages, .. } = cargo_metadata::MetadataCommand::new()
        .no_deps()
        .exec()
        .context("MetadataCommand::exec")?;
    let ebpf_package = packages
        .into_iter()
        .find(|cargo_metadata::Package { name, .. }| name.as_str() == "kret-ebpf")
        .ok_or_else(|| anyhow!("kret-ebpf package not found"))?;
    let cargo_metadata::Package {
        name,
        manifest_path,
        ..
    } = ebpf_package;

    let target_feature = if cfg!(feature = "riscv64") {
        "riscv64"
    } else if cfg!(feature = "loongarch64") {
        "loongarch64"
    } else {
        "x86_64"
    };

    let ebpf_package = aya_build::Package {
        name: name.as_str(),
        root_dir: manifest_path
            .parent()
            .ok_or_else(|| anyhow!("no parent for {manifest_path}"))?
            .as_str(),
        features: &[target_feature],
        ..Default::default()
    };
    aya_build::build_ebpf([ebpf_package], Toolchain::default())
}
