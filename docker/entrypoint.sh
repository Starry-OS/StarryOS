#!/bin/bash
# Fix line endings for shell scripts
find /workspace -type f -name "*.sh" -exec dos2unix {} + || true
# Execute the command passed to the container
exec "$@"

