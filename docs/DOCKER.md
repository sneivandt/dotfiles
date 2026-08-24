# Docker

The multi-stage Ubuntu 24.04 image builds the Rust CLI and applies the selected
dotfiles profile to a non-root user.

## Build

BuildKit is required because the Dockerfile uses cache mounts:

```bash
docker build --build-arg PROFILE=base -t dotfiles:local .
```

For desktop-category configuration:

```bash
docker build --build-arg PROFILE=desktop -t dotfiles:desktop .
```

The `PROFILE` build argument defaults to `base`.

## Image construction

The builder stage:

1. Installs Rust and native build dependencies.
2. Copies Git metadata.
3. Exports the committed source with `git archive`.
4. Sanitizes repository authentication metadata.
5. Builds and strips the release binary.

The runtime stage:

1. Installs a small Ubuntu command-line environment.
2. Configures `en_US.UTF-8`.
3. Creates the non-root `sneivandt` user with Zsh.
4. Copies source, sanitized Git metadata, and the binary.
5. Runs `dotfiles install` with the selected profile as that user. This build
   step sets `DOTFILES_SKIP_SELF_UPDATE=1` so it uses the binary compiled from
   the checked-out commit instead of consulting the latest release.
6. Starts Zsh by default.

The runtime image retains `.git` so sparse-checkout and repository-update tasks
still work. Its origin points to the public HTTPS repository, and the build
removes credential-bearing Git headers.

## Run

```bash
docker run --rm -it dotfiles:local
```

Inspect the installed CLI:

```bash
docker run --rm dotfiles:local dotfiles --version
```

## Version metadata

Set `DOTFILES_VERSION` to override the version. Without it, the builder uses
`git describe` to select the nearest reachable tag matching `v[0-9]*`:

```bash
docker build \
  --build-arg DOTFILES_VERSION=v1.2.3 \
  --build-arg PROFILE=base \
  -t dotfiles:v1.2.3 .
```

The checkout must contain the required Git metadata and committed source.
Uncommitted working-tree changes are not included because the Dockerfile uses
`git archive HEAD`.

## CI publishing

After CI succeeds for a same-repository push to `main`, the Docker publishing
workflow checks out that CI run's head SHA, then builds and pushes `latest` and
an immutable `sha-<commit>` tag. A newer publish cancels an older in-progress
publish so an obsolete build cannot overwrite `latest` later.

## Limitations

- The image is Ubuntu, not Arch; Arch-only package and AUR tasks are not
  applicable.
- Host desktop services and Windows registry configuration are not represented.
- Installation occurs at image build time, so changing profile or configuration
  requires rebuilding.
- Private overlays are not copied into the public image.
- Use the image for environment validation or as a shell. It does not emulate
  every supported platform.
