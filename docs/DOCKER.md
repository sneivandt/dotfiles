# Docker

Using the dotfiles with Docker for isolated testing and development environments.

## Quick Start

### Run Published Image

The easiest way to try the dotfiles in an isolated environment:

```bash
docker run --rm -it sneivandt/dotfiles
```

This drops you into a shell with the dotfiles pre-installed using the `base` profile.

### Build Local Image

Build the image from the current repository:

```bash
docker buildx build -t dotfiles:local .
docker run --rm -it dotfiles:local
```

## Published Image

The official image is automatically built and published to Docker Hub:
- **Repository**: [sneivandt/dotfiles](https://hub.docker.com/r/sneivandt/dotfiles)
- **Trigger**: Pushes to `master` branch
- **Workflow**: `.github/workflows/docker-image.yml`

### Pulling the Image

```bash
# Pull latest version
docker pull sneivandt/dotfiles

# Pull specific tag (if tagged)
docker pull sneivandt/dotfiles:v1.0.0
```

## Dockerfile Overview

The Dockerfile uses a multi-stage build:

1. **Builder stage**: Compiles the Rust binary from `cli/` using cargo
2. **Runtime stage**: Ubuntu-based image with the pre-compiled binary

```dockerfile
# Stage 1: Build the Rust binary
FROM ubuntu:latest AS builder
RUN apt-get update && apt-get install -y ca-certificates curl git
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
WORKDIR /build
COPY cli/ cli/
COPY .git .git
RUN cargo build --release --manifest-path cli/Cargo.toml && strip cli/target/release/dotfiles

# Stage 2: Runtime image
FROM ubuntu:latest
RUN apt-get update && apt-get install -y git vim zsh tmux ...
RUN useradd -m -s /bin/zsh sneivandt
COPY --chown=sneivandt:sneivandt conf /home/sneivandt/dotfiles/conf
COPY --chown=sneivandt:sneivandt symlinks /home/sneivandt/dotfiles/symlinks
COPY --from=builder /build/cli/target/release/dotfiles /home/sneivandt/dotfiles/bin/dotfiles
USER sneivandt
RUN /home/sneivandt/dotfiles/bin/dotfiles --root /home/sneivandt/dotfiles -p base install
CMD ["/usr/bin/zsh"]
```

See the actual [`Dockerfile`](../Dockerfile) for the full implementation with BuildKit cache mounts and locale configuration.

## Use Cases

### Testing Configuration Changes

Test your configuration changes in a clean environment:

```bash
# Build with your changes
docker buildx build -t dotfiles:test .

# Run and verify
docker run --rm -it dotfiles:test

# Inside container, test commands
zsh
nvim
# etc.
```

### CI/CD Integration

Use the Docker image in your CI pipeline:

```yaml
# .github/workflows/example.yml
jobs:
  test:
    runs-on: ubuntu-latest
    container:
      image: sneivandt/dotfiles
    steps:
      - name: Run tests
        run: |
          # Your tests here
```

### Development Environment

Create a consistent development environment:

```bash
# Run with volume mount for persistence
docker run --rm -it -v ~/projects:/projects sneivandt/dotfiles

# Work in mounted directory
cd /projects
# Your development work here
```

### Profile Testing

Test different profiles in isolation:

```bash
# Test base profile
docker buildx build --build-arg PROFILE=base -t dotfiles:base .
docker run --rm -it dotfiles:base

# Test desktop profile
docker buildx build --build-arg PROFILE=desktop -t dotfiles:desktop .
docker run --rm -it dotfiles:desktop
```

## Building Variants

### Custom Base Image

Use a different base image:

```dockerfile
FROM ubuntu:22.04

# Adapt package installation for Ubuntu
RUN apt-get update && apt-get install -y git

# Rest of setup...
```

### Minimal Image

Create a minimal image with base profile:

```dockerfile
FROM archlinux:latest

RUN pacman -Syu --noconfirm && \
    pacman -S --noconfirm git sudo

RUN useradd -m -s /bin/bash dotfiles && \
    echo '%wheel ALL=(ALL) NOPASSWD: ALL' >> /etc/sudoers

USER dotfiles
WORKDIR /home/dotfiles
RUN git clone https://github.com/sneivandt/dotfiles.git && \
    cd dotfiles && \
    ./dotfiles.sh install -p base

WORKDIR /home/dotfiles/dotfiles
CMD ["/bin/bash"]
```

### Development Image

Add development tools:

```dockerfile
FROM archlinux:latest

# Install base system
RUN pacman -Syu --noconfirm && \
    pacman -S --noconfirm git base-devel sudo nodejs npm python go

# Setup user
RUN useradd -m -G wheel -s /bin/bash dotfiles && \
    echo '%wheel ALL=(ALL) NOPASSWD: ALL' >> /etc/sudoers

# Install dotfiles
USER dotfiles
WORKDIR /home/dotfiles
RUN git clone https://github.com/sneivandt/dotfiles.git && \
    cd dotfiles && \
    ./dotfiles.sh install -p desktop

WORKDIR /home/dotfiles
CMD ["/bin/zsh"]
```

## Advanced Usage

### Multi-Stage Build

Optimize image size with multi-stage builds:

```dockerfile
# Stage 1: Build environment
FROM archlinux:latest AS builder

RUN pacman -Syu --noconfirm
RUN pacman -S --noconfirm git base-devel

RUN useradd -m -s /bin/bash dotfiles
USER dotfiles
WORKDIR /home/dotfiles

RUN git clone https://github.com/sneivandt/dotfiles.git
WORKDIR /home/dotfiles/dotfiles
RUN ./dotfiles.sh install -p base

# Stage 2: Runtime
FROM archlinux:latest

RUN pacman -Syu --noconfirm && \
    pacman -S --noconfirm git

RUN useradd -m -s /bin/bash dotfiles
USER dotfiles

COPY --from=builder /home/dotfiles/.bashrc /home/dotfiles/
COPY --from=builder /home/dotfiles/.vimrc /home/dotfiles/
# Copy other needed files

CMD ["/bin/bash"]
```

### Build Arguments

Use build arguments for flexibility:

```dockerfile
ARG PROFILE=desktop
ARG DOTFILES_REPO=https://github.com/sneivandt/dotfiles.git

FROM archlinux:latest

# ... system setup ...

RUN git clone ${DOTFILES_REPO} dotfiles
WORKDIR /home/dotfiles/dotfiles
RUN ./dotfiles.sh install -p ${PROFILE}
```

Build with custom arguments:
```bash
docker buildx build \
  --build-arg PROFILE=base \
  --build-arg DOTFILES_REPO=https://github.com/youruser/dotfiles.git \
  -t dotfiles:custom .
```

### Persistent Storage

Run with persistent home directory:

```bash
# Create volume
docker volume create dotfiles-home

# Run with volume
docker run --rm -it -v dotfiles-home:/home/dotfiles sneivandt/dotfiles

# Changes persist across container restarts
```

### Networking

Run with host networking for development servers:

```bash
docker run --rm -it --network host sneivandt/dotfiles
```

### GPU Access (for GUI testing)

Enable GPU access for testing desktop configurations:

```bash
docker run --rm -it \
  --gpus all \
  -e DISPLAY=$DISPLAY \
  -v /tmp/.X11-unix:/tmp/.X11-unix \
  sneivandt/dotfiles
```

## Docker Compose

Use Docker Compose for complex setups:

```yaml
# docker-compose.yml
version: '3.8'

services:
  dotfiles:
    image: sneivandt/dotfiles
    volumes:
      - dotfiles-home:/home/dotfiles
      - ./projects:/projects
    environment:
      - TERM=xterm-256color
    stdin_open: true
    tty: true

volumes:
  dotfiles-home:
```

Run with:
```bash
docker-compose up -d
docker-compose exec dotfiles bash
```

## Troubleshooting

### Build Fails During Package Installation

**Problem**: pacman fails to install packages.

**Solution**:
```dockerfile
# Update mirrors and refresh database
RUN pacman -Syy --noconfirm
RUN pacman -Syu --noconfirm
```

### Permission Issues

**Problem**: Permission denied errors.

**Solution**:
```dockerfile
# Ensure user has proper permissions
RUN useradd -m -G wheel -s /bin/bash dotfiles
RUN echo '%wheel ALL=(ALL) NOPASSWD: ALL' >> /etc/sudoers
USER dotfiles
```

### Image Size Too Large

**Problem**: Docker image is too large.

**Solutions**:
1. Use multi-stage builds
2. Clean package cache:
   ```dockerfile
   RUN pacman -Scc --noconfirm
   ```
3. Remove build dependencies after installation
4. Use `.dockerignore` to exclude unnecessary files

### Container Exits Immediately

**Problem**: Container exits after starting.

**Solution**:
```dockerfile
# Use interactive shell as CMD
CMD ["/bin/bash"]
```

And run with `-it` flags:
```bash
docker run --rm -it sneivandt/dotfiles
```

## Best Practices

1. **Layer Caching**: Order Dockerfile commands from least to most frequently changed
2. **Clean Up**: Remove package caches and temporary files
3. **User Context**: Run as non-root user when possible
4. **Build Arguments**: Use arguments for flexibility
5. **Multi-Stage**: Use multi-stage builds for smaller images
6. **Version Tags**: Tag images with version numbers
7. **Documentation**: Document custom Dockerfile modifications

## CI/CD Integration

### GitHub Actions

The project includes automated Docker image publishing:

```yaml
# .github/workflows/docker-image.yml
name: Publish Docker

on:
  push:
    branches: [ master ]

jobs:
  push_to_registry:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: docker/build-push-action@v4
        with:
          push: true
          tags: sneivandt/dotfiles:latest
```

### GitLab CI

Example GitLab CI configuration:

```yaml
# .gitlab-ci.yml
docker-build:
  stage: build
  image: docker:latest
  services:
    - docker:dind
  script:
    - docker build -t $CI_REGISTRY_IMAGE:$CI_COMMIT_SHA .
    - docker push $CI_REGISTRY_IMAGE:$CI_COMMIT_SHA
```

## Examples

### Quick Test Environment

```bash
# Spin up test environment
docker run --rm -it sneivandt/dotfiles

# Test commands
git --version
nvim --version
zsh --version
```

### Development Session

```bash
# Start development container with project mount
docker run --rm -it \
  -v ~/myproject:/project \
  -w /project \
  sneivandt/dotfiles

# Work in your project with dotfiles configuration
cd /project
vim main.go
```

### Batch Testing

```bash
# Test multiple profiles
for profile in base desktop; do
  echo "Testing $profile..."
  docker buildx build \
    --build-arg PROFILE=$profile \
    -t dotfiles:$profile \
    .
  docker run --rm dotfiles:$profile /bin/bash -c "echo Profile $profile works"
done
```

## See Also

- [Architecture](ARCHITECTURE.md) - Implementation details
- [Testing](TESTING.md) - Testing procedures
- [Profile System](PROFILES.md) - Understanding profiles
- [Dockerfile](../Dockerfile) - Image definition
- [Docker Hub Repository](https://hub.docker.com/r/sneivandt/dotfiles)
