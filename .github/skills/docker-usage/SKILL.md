---
name: docker-usage
description: >
  Using Docker for testing and development with the dotfiles project.
  Use when working with Docker images, containers, or CI/CD integration.
metadata:
  author: sneivandt
  version: "1.0"
---

# Docker Usage

This skill documents how to use Docker with the dotfiles project for isolated testing, development, and CI/CD integration.

## Overview

The dotfiles project provides a Docker image for:
- **Testing**: Verify configuration changes in clean environment
- **Development**: Consistent environment across machines
- **CI/CD**: Automated testing in GitHub Actions
- **Profile Testing**: Test different profiles in isolation

## Published Image

The official image is automatically built and published to Docker Hub:
- **Repository**: [sneivandt/dotfiles](https://hub.docker.com/r/sneivandt/dotfiles)
- **Base**: Ubuntu latest with base profile pre-installed
- **Trigger**: Pushes to `master` branch
- **Workflow**: `.github/workflows/docker-image.yml`

### Quick Start

```bash
# Pull and run the latest image
docker pull sneivandt/dotfiles
docker run --rm -it sneivandt/dotfiles

# Or run directly (pulls automatically)
docker run --rm -it sneivandt/dotfiles
```

This drops you into a zsh shell with the dotfiles pre-installed using the `base` profile.

## Dockerfile Structure

The Dockerfile uses multi-stage optimizations and best practices:

```dockerfile
FROM ubuntu:latest

# Install system packages (with BuildKit cache)
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    apt-get update && apt-get install -y \
    git vim zsh tmux curl

# Configure locale
RUN echo "en_US.UTF-8 UTF-8" > /etc/locale.gen && locale-gen

# Add non-root user
RUN useradd -m -s /bin/zsh sneivandt
WORKDIR /home/sneivandt

# Copy dotfiles repository
COPY --chown=sneivandt:sneivandt .git /home/sneivandt/dotfiles/.git
COPY --chown=sneivandt:sneivandt conf /home/sneivandt/dotfiles/conf
COPY --chown=sneivandt:sneivandt src /home/sneivandt/dotfiles/src
COPY --chown=sneivandt:sneivandt symlinks /home/sneivandt/dotfiles/symlinks
COPY --chown=sneivandt:sneivandt dotfiles.sh /home/sneivandt/dotfiles/dotfiles.sh

# Install dotfiles as user
USER sneivandt
RUN /home/sneivandt/dotfiles/dotfiles.sh --install --profile base \
    && rm -rf /home/sneivandt/dotfiles/.git

CMD ["/usr/bin/zsh"]
```

## .dockerignore File

The `.dockerignore` file uses an allowlist approach for security:

```
# Exclude everything by default
*

# Include only what's needed for build
!.git
!conf
!src
!symlinks
!test
!dotfiles.sh
```

This prevents accidentally including:
- Build artifacts
- Environment files (.env)
- IDE configurations
- Local development files
- Sensitive data

## Building Local Images

### Basic Build

```bash
# Build from current directory
docker buildx build -t dotfiles:local .

# Run the built image
docker run --rm -it dotfiles:local
```

### Build with Specific Profile

Modify Dockerfile to use different profile:

```dockerfile
# Change this line:
RUN /home/sneivandt/dotfiles/dotfiles.sh --install --profile base

# To:
RUN /home/sneivandt/dotfiles/dotfiles.sh --install --profile desktop
```

Or use build arguments:

```dockerfile
ARG PROFILE=base
RUN /home/sneivandt/dotfiles/dotfiles.sh --install --profile ${PROFILE}
```

Then build with:
```bash
docker buildx build --build-arg PROFILE=desktop -t dotfiles:desktop .
```

### Build with Cache

Use BuildKit cache mounts for faster rebuilds:

```bash
# Enable BuildKit
export DOCKER_BUILDKIT=1

# Build with cache
docker buildx build --cache-from=type=local,src=/tmp/.buildx-cache \
                    --cache-to=type=local,dest=/tmp/.buildx-cache \
                    -t dotfiles:cached .
```

## Use Cases

### Testing Configuration Changes

Test changes in a clean environment before committing:

```bash
# Make changes to dotfiles
vim symlinks/config/myapp/config.yml
vim conf/symlinks.ini

# Build test image
docker buildx build -t dotfiles:test .

# Run and verify changes
docker run --rm -it dotfiles:test

# Inside container
ls -la ~/.config/myapp/
cat ~/.config/myapp/config.yml
```

### Profile Testing

Test different profiles in isolation:

```bash
# Test base profile (minimal)
docker buildx build --build-arg PROFILE=base -t dotfiles:base .
docker run --rm -it dotfiles:base

# Test desktop profile (with GUI configs)
docker buildx build --build-arg PROFILE=desktop -t dotfiles:desktop .
docker run --rm -it dotfiles:desktop
```

### Development Environment

Create a consistent development environment with volume mounts:

```bash
# Mount host directory for persistence
docker run --rm -it \
  -v ~/projects:/projects \
  -v ~/.ssh:/home/sneivandt/.ssh:ro \
  sneivandt/dotfiles

# Inside container
cd /projects
git status
nvim somefile.txt
```

### CI/CD Integration

Use the Docker image in GitHub Actions:

```yaml
# .github/workflows/example.yml
jobs:
  test:
    runs-on: ubuntu-latest
    container:
      image: sneivandt/dotfiles
    steps:
      - uses: actions/checkout@v4

      - name: Run tests
        run: |
          ./run-tests.sh

      - name: Verify configs
        run: |
          ls -la ~/.config/
          zsh --version
```

### Dry-Run Testing

Test installation without making changes:

```bash
# Build image with dry-run
docker buildx build -t dotfiles:dryrun .

# Run with dry-run flag
docker run --rm -it dotfiles:dryrun \
  bash -c "/home/sneivandt/dotfiles/dotfiles.sh --install --dry-run"
```

## Building Variants

### Minimal Image

Create a minimal image with only essential tools:

```dockerfile
FROM ubuntu:latest
ENV DEBIAN_FRONTEND=noninteractive

# Minimal packages only
RUN apt-get update && apt-get install -y \
    git \
    zsh \
    && rm -rf /var/lib/apt/lists/*

# Add user and install dotfiles
RUN useradd -m -s /bin/zsh sneivandt
WORKDIR /home/sneivandt
COPY --chown=sneivandt:sneivandt . /home/sneivandt/dotfiles/
USER sneivandt
RUN /home/sneivandt/dotfiles/dotfiles.sh --install --profile base

CMD ["/usr/bin/zsh"]
```

### Arch Linux Variant

Create an Arch Linux based image:

```dockerfile
FROM archlinux:latest

# Update system and install base packages
RUN pacman -Syu --noconfirm && \
    pacman -S --noconfirm git zsh vim tmux base-devel

# Add user
RUN useradd -m -s /bin/zsh sneivandt
WORKDIR /home/sneivandt

# Install dotfiles
COPY --chown=sneivandt:sneivandt . /home/sneivandt/dotfiles/
USER sneivandt
RUN /home/sneivandt/dotfiles/dotfiles.sh --install --profile arch

CMD ["/usr/bin/zsh"]
```

### Development Image with Additional Tools

Add development tools to the image:

```dockerfile
FROM sneivandt/dotfiles

USER root
RUN apt-get update && apt-get install -y \
    build-essential \
    python3-pip \
    nodejs \
    npm \
    && rm -rf /var/lib/apt/lists/*

USER sneivandt
RUN pip3 install --user pipenv poetry
```

## Docker Compose

Use Docker Compose for complex setups:

```yaml
# docker-compose.yml
version: '3.8'

services:
  dotfiles:
    build: .
    image: dotfiles:local
    volumes:
      - ~/projects:/projects
      - ~/.ssh:/home/sneivandt/.ssh:ro
    working_dir: /projects
    stdin_open: true
    tty: true
    command: /usr/bin/zsh
```

Run with:
```bash
docker compose up -d
docker compose exec dotfiles zsh
```

## Rules for Docker Usage

1. **Use official base images**: Start with `ubuntu:latest` or `archlinux:latest` for consistency

2. **Non-root user**: Always create and use a non-root user for security

3. **Layer caching**: Order Dockerfile commands from least to most frequently changing

4. **BuildKit cache**: Use cache mounts for package managers to speed up rebuilds

5. **Clean up**: Remove package manager caches and .git directory to reduce image size

6. **Allowlist .dockerignore**: Exclude everything by default, then include only needed files

7. **Pin versions**: Consider pinning base image versions for reproducibility (e.g., `ubuntu:22.04` instead of `latest`)

8. **Test locally first**: Always test Docker builds locally before pushing to CI/CD

9. **Volume mounts for development**: Use volumes for persistent data and SSH keys

10. **Profile selection**: Use build arguments for profile selection to maintain a single Dockerfile

## Troubleshooting

### Build Fails on Package Installation

**Issue**: Package installation fails during build

**Solutions**:
- Update package lists: `apt-get update` or `pacman -Syu`
- Check package names are correct
- Use `--no-install-recommends` to reduce dependencies
- Add cache mount to speed up retries

### Container Exits Immediately

**Issue**: Container starts then exits

**Solutions**:
- Ensure CMD is interactive: `CMD ["/usr/bin/zsh"]`
- Run with `-it` flags: `docker run -it`
- Check for errors in entrypoint script

### Dotfiles Installation Fails

**Issue**: dotfiles.sh fails during build

**Solutions**:
- Check `.dockerignore` includes all required files
- Verify file permissions are correct
- Run with `--dry-run` flag to see what would happen
- Check logs in the container: `docker logs <container-id>`

## Cross-References

- See the `profile-system` skill for profile selection and filtering
- See the `testing-patterns` skill for testing approaches
- See the `.github/workflows/docker-image.yml` for CI/CD example
