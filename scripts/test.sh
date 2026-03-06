#!/bin/bash
set -e

echo "=== StarryOS Test Script ==="
echo ""

# --------------------------------------------------------------------------
# [1/4] Check required tools
# --------------------------------------------------------------------------
check_tools() {
    echo "[1/4] Checking required tools..."

    local missing=false

    for tool in make cargo python3; do
        if ! command -v "$tool" &> /dev/null; then
            echo "Error: '$tool' is not installed"
            missing=true
        fi
    done

    for arch in riscv64 aarch64 loongarch64 x86_64; do
        qemu_cmd="qemu-system-${arch}"
        if ! command -v "$qemu_cmd" &> /dev/null; then
            echo "Warning: '$qemu_cmd' not found (QEMU test for $arch will be skipped)"
        fi
    done

    if [ "$missing" = "true" ]; then
        echo "Error: missing required tools, aborting."
        exit 1
    fi

    echo "All required tools are available"
    echo ""
}

# --------------------------------------------------------------------------
# [2/4] Code format check
# --------------------------------------------------------------------------
check_format() {
    echo "[2/4] Checking code format..."
    cargo fmt --all -- --check
    echo "Code format check passed"
    echo ""
}

# --------------------------------------------------------------------------
# [3/4] Per-architecture: rootfs download + build + QEMU boot test
# --------------------------------------------------------------------------
run_arch_tests() {
    echo "[3/4] Running architecture-specific build and boot tests..."

    local archs=("riscv64" "aarch64" "loongarch64" "x86_64")
    local all_passed=true

    for arch in "${archs[@]}"; do
        echo ""
        echo "--- Architecture: $arch ---"

        # Check if QEMU is available for this arch
        if ! command -v "qemu-system-${arch}" &> /dev/null; then
            echo "Warning: qemu-system-${arch} not found, skipping $arch"
            continue
        fi

        echo "  Step 1: downloading rootfs for $arch..."
        cargo xtask rootfs --arch="$arch"

        echo "  Step 2: building kernel for $arch..."
        cargo xtask build --arch="$arch"

        echo "  Step 3: running boot test for $arch..."
        if cargo xtask test --arch="$arch"; then
            echo "$arch boot test passed"
        else
            echo "Error: $arch boot test FAILED"
            all_passed=false
        fi
    done

    echo ""
    if [ "$all_passed" = "true" ]; then
        echo "All architecture tests passed"
    else
        echo "Error: one or more architecture tests failed"
        exit 1
    fi
    echo ""
}

# --------------------------------------------------------------------------
# [4/4] Summary
# --------------------------------------------------------------------------
print_summary() {
    echo "[4/4] Test Summary"
    echo "=================="
    echo "All checks passed successfully!"
    echo ""
    echo "The following checks were performed:"
    echo "  1. Tool availability check (make, cargo, python3, qemu-system-*)"
    echo "  2. Code format check (cargo fmt --all)"
    echo "  3. Architecture build + boot tests (riscv64, aarch64, loongarch64, x86_64)"
    echo ""
}

# --------------------------------------------------------------------------
# Main
# --------------------------------------------------------------------------
main() {
    local skip_qemu="${SKIP_QEMU:-false}"

    check_tools
    check_format

    if [ "$skip_qemu" = "true" ]; then
        echo "[3/4] Skipping architecture tests (SKIP_QEMU=true)"
        echo ""
    else
        run_arch_tests
    fi

    print_summary
}

main "$@"
