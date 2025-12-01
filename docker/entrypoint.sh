#!/bin/bash

# Entry point for the StarryOS Docker image.
# We intentionally do NOT modify files in /workspace here,
# to avoid making the mounted repository look "dirty" to users.

# Execute the command passed to the container
exec "$@"

