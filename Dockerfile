# syntax=docker/dockerfile:1
FROM rust:1.95.0-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1 AS builder

ENV CARGO_TARGET_DIR=/build/target

WORKDIR /build
COPY .git .git
RUN mkdir -p /build/source \
    && git archive --format=tar HEAD | tar -x -C /build/source \
    && (git --git-dir=/build/.git config --unset-all http.https://github.com/.extraheader || true) \
    && (git --git-dir=/build/.git remote remove origin || true) \
    && git --git-dir=/build/.git remote add origin https://github.com/sneivandt/dotfiles.git \
    && git --git-dir=/build/.git --work-tree=/build/source checkout -B main HEAD \
    && git --git-dir=/build/.git update-ref refs/remotes/origin/main HEAD \
    && git --git-dir=/build/.git branch --set-upstream-to=origin/main main
WORKDIR /build/source
ARG DOTFILES_VERSION
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git/db,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    version="${DOTFILES_VERSION:-$(git --git-dir=/build/.git describe --tags --abbrev=0 --match 'v[0-9]*')}" \
    && DOTFILES_VERSION="$version" cargo build --release --locked --manifest-path cli/Cargo.toml \
    && strip /build/target/release/dotfiles \
    && install -D -m 0755 /build/target/release/dotfiles /build/out/dotfiles

FROM ubuntu:24.04@sha256:33ceb71981b602c1a7443a53469e4dba065f7503eab3078a2d7a57a2ab987517
ARG PROFILE=base

LABEL org.opencontainers.image.title="dotfiles" \
      org.opencontainers.image.description="Cross-platform dotfiles for Linux/Arch/Windows" \
      org.opencontainers.image.authors="Stuart Neivandt" \
      org.opencontainers.image.url="https://github.com/sneivandt/dotfiles" \
      org.opencontainers.image.source="https://github.com/sneivandt/dotfiles" \
      org.opencontainers.image.documentation="https://github.com/sneivandt/dotfiles/blob/main/README.md" \
      org.opencontainers.image.licenses="MIT"

ENV DEBIAN_FRONTEND=noninteractive

# Install packages (use BuildKit cache mount for faster rebuilds)
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update \
    && apt-get install --no-install-recommends --no-install-suggests -y \
        ca-certificates \
        curl \
        exuberant-ctags \
        git \
        locales \
        openssh-client \
        tmux \
        vim \
        wget \
        zip \
        zsh

# Configure locale
RUN echo "en_US.UTF-8 UTF-8" > /etc/locale.gen \
    && locale-gen

# Add user (let system assign UID to avoid conflicts)
RUN useradd -m -s /bin/zsh -U sneivandt
WORKDIR /home/sneivandt
ENV SHELL=/bin/zsh \
    USER=sneivandt \
    LOGNAME=sneivandt \
    PATH=/home/sneivandt/.local/bin:${PATH}

# Install a self-managing dotfiles checkout. Keep sanitized Git metadata so
# update and sparse-checkout tasks can operate inside the image.
COPY --from=builder --chown=sneivandt:sneivandt /build/source/ /home/sneivandt/dotfiles/
COPY --from=builder --chown=sneivandt:sneivandt /build/.git /home/sneivandt/dotfiles/.git
COPY --from=builder --chown=sneivandt:sneivandt /build/out/dotfiles /home/sneivandt/dotfiles/bin/dotfiles
USER sneivandt
RUN DOTFILES_SKIP_SELF_UPDATE=1 \
    /home/sneivandt/dotfiles/bin/dotfiles install --root /home/sneivandt/dotfiles -p "$PROFILE"
CMD ["/usr/bin/zsh"]
