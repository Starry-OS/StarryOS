fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    std::fs::write(
        format!("{}/kallsyms.ld", out_dir),
        include_str!("kallsym.ld"),
    )
    .unwrap();
    println!(
        "cargo:rustc-link-arg=-T{}",
        format!("{}/kallsyms.ld", out_dir)
    );
}
