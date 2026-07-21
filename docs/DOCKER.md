# Docker

The repository includes a multi-stage Ubuntu 24.04 image that builds the Rust
CLI and applies the dotfiles configuration to a non-root user during image
construction.

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
5. Runs `dotfiles install` with the selected profile as that user.
6. Starts Zsh by default.

The retained `.git` directory allows sparse-checkout and repository-update tasks
to operate in the image. The origin is reset to the public HTTPS repository and
credential-bearing Git headers are removed.

## Run

```bash
docker run --rm -it dotfiles:local
```

Inspect the installed CLI:

```bash
docker run --rm dotfiles:local dotfiles --version
```

## Version metadata

The builder accepts `DOTFILES_VERSION`. When omitted, it derives the version
from the latest matching `v*` Git tag:

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

The Docker publishing workflow runs after successful CI for a
same-repository push to `main`. It checks out the exact successful CI head SHA
before building and pushing. This keeps the published image tied to the tested
commit.

## Limitations

- The image is Ubuntu, not Arch; Arch-only package and AUR tasks are not
  applicable.
- Host desktop services and Windows registry configuration are not represented.
- Installation occurs at image build time, so changing profile or configuration
  requires rebuilding.
- Private overlays are not copied into the public image.
- The image is an environment validation and shell image, not a full virtual
  machine for every supported platform.
