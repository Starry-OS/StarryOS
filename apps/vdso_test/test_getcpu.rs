use std::println;

fn main() {
    println!("test_getcpu: calling init_vdso_getcpu(cpu=1, node=0)");

    // Call the vDSO getcpu initializer from the library.
    // This may require appropriate privileges on the running machine.
    // If running on a non-x86_64 platform this binary will not call the
    // x86_64 implementation because the crate exposes platform modules
    // conditionally.
    if cfg!(target_arch = "x86_64") {
        // Use the crate name with hyphen turned to underscore
        use starry_vdso::x86_64::getcpu::init_vdso_getcpu;

        // Example values; adjust as needed for your test environment.
        init_vdso_getcpu(1u32, 0u32);
        println!("init_vdso_getcpu returned (check kernel/log output for details)");
    } else {
        println!("Skipping init_vdso_getcpu: not x86_64 target");
    }
}
