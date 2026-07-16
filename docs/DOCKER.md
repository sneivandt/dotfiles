# Docker

The Docker image provides an isolated Ubuntu environment with the selected
dotfiles profile installed for the non-root `sneivandt` user.

The [`Dockerfile`](../Dockerfile) is the source of truth for the image contents
and build process. The [Docker workflow](../.github/workflows/docker.yml) is the
source of truth for publication.

## Run the published image

```bash
docker pull sneivandt/dotfiles:latest
docker run --rm -it sneivandt/dotfiles:latest
```

The workflow currently publishes only the `latest` tag after a successful CI
run for a same-repository push to `main`. Versioned image tags are not
published.

## Build locally

BuildKit is required because the Dockerfile uses cache mounts:

```bash
docker buildx build --load -t dotfiles:local .
docker run --rm -it dotfiles:local
```

The default profile is `base`. Select another profile with the `PROFILE` build
argument; valid profiles are defined in
[`conf/profiles.toml`](../conf/profiles.toml):

```bash
docker buildx build --load --build-arg PROFILE=desktop -t dotfiles:desktop .
docker run --rm -it dotfiles:desktop
```

To test a specific application version independently of Git tag discovery,
pass `DOTFILES_VERSION`:

```bash
docker buildx build --load --build-arg DOTFILES_VERSION=v0.1.0 -t dotfiles:local .
```

## What the image contains

The multi-stage build:

1. Archives the checked-out commit into a clean build context and compiles the
   Rust CLI in an Ubuntu 24.04 builder.
2. Sanitizes and preserves Git metadata so repository update and
   sparse-checkout tasks continue to work in the runtime image.
3. Installs the runtime tools declared in the Dockerfile, creates the
   non-root `sneivandt` user, and runs `dotfiles install` for the selected
   profile.
4. Starts `/usr/bin/zsh`.

Inspect the current [`Dockerfile`](../Dockerfile) rather than copying a
Dockerfile fragment from this guide.

## Use a host workspace

Mount a host directory when the container should operate on persistent files:

```bash
docker run --rm -it -v "$PWD:/workspace" -w /workspace dotfiles:local
```

The dotfiles checkout inside the image remains at
`/home/sneivandt/dotfiles`.

## Troubleshooting

If cache mounts are rejected, use `docker buildx build` as shown above or enable
BuildKit for the Docker daemon.

To inspect a failed install layer:

```bash
docker buildx build --load --progress=plain -t dotfiles:debug .
```

To inspect the installed environment without the default shell command:

```bash
docker run --rm -it --entrypoint /bin/bash dotfiles:local
```
