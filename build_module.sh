#!/bin/bash

set -e

MODULE_NAME="$1"
TARGET="$2"
MODULE_BUILD_DIR="$3"
BUILD_DIR="$4"
LD_COMMAND="$5"

if [ -z "$MODULE_NAME" ] || [ -z "$TARGET" ] || [ -z "$MODULE_BUILD_DIR" ]; then
    echo "Usage: $0 <module_name> <target> <module_build_dir> <build_dir> <ld_command>"
    exit 1
fi

BUILD_DIR="${BUILD_DIR:=target}"
LD_COMMAND="${LD_COMMAND:=ld}"

lib_path="${MODULE_BUILD_DIR}/lib${MODULE_NAME}.a"
if [ ! -f "$lib_path" ]; then
    echo "Error: Library not found: $lib_path"
    exit 1
fi

lib_path_abs="$(cd "$(dirname "$lib_path")" && pwd)/$(basename "$lib_path")"


output_ko="${BUILD_DIR}/${MODULE_NAME}/${MODULE_NAME}.ko"
mkdir -p "$(dirname "$output_ko")"

echo "Linking kernel module $MODULE_NAME"
echo "Linking --whole-archive $lib_path to $output_ko"

# no-pie: Position Independent Executable is not supported in kernel modules
LD_FLAGS="--strip-debug --build-id=none --gc-sections -no-pie"

$LD_COMMAND -r -T linker.ld -o "$output_ko" --whole-archive $lib_path_abs $LD_FLAGS

if [ $? -ne 0 ]; then
    echo "Failed to create kernel module for $MODULE_NAME"
    exit 1
fi


if [ -f "$output_ko" ]; then
    size=$(stat -c%s "$output_ko" 2>/dev/null || stat -f%z "$output_ko" 2>/dev/null)
    echo "Successfully created kernel module: $output_ko"
    echo "Module size: $size bytes"
else
    echo "Error: Kernel module file was not created"
    exit 1
fi

exit 0
