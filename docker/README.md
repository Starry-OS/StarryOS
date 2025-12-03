# Docker Build Environment

This directory contains Docker-related files for building StarryOS in a consistent Linux environment.

- **Tip**: When running QEMU inside Docker, you may need extra options for hardware acceleration and networking (for example, `--device /dev/kvm` and `--cap-add=NET_ADMIN`). For more details, please refer to the official QEMU / Docker documentation.

## Files

- `Dockerfile` - Main Docker image definition
- `docker-compose.yml` - Optional Docker Compose configuration (for convenience)

## Usage

### Using Docker directly (Recommended)

From the project root:

```bash
# Build the Docker image
$ docker build -t localhost/starry-env -f docker/Dockerfile .

# Run the container
$ docker run -it --rm -v .:/workspace localhost/starry-env
```

### Using Docker Compose (Optional)

Docker Compose is provided as an optional convenience tool. From the project root:

```bash
# Build and start the container
$ docker-compose -f docker/docker-compose.yml up -d

# Enter the container
$ docker-compose -f docker/docker-compose.yml exec starryos bash

# Stop the container
$ docker-compose -f docker/docker-compose.yml down
```

**Note:** Docker Compose is primarily designed for deploying applications. For development environments, using Docker commands directly is recommended and more straightforward.

## Inside the Container

Once inside the container, you can build and run StarryOS:

```bash
$ make build
$ make img
$ make run ARCH=riscv64
```
