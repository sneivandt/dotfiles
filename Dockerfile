# syntax=docker/dockerfile:1
FROM ubuntu:24.04 AS builder

ENV DEBIAN_FRONTEND=noninteractive

# Install Rust and build dependencies
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update \
    && apt-get install --no-install-recommends --no-install-suggests -y \
        build-essential \
        ca-certificates \
        curl \
        git \
        libssl-dev \
        pkg-config

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build
COPY .git .git
RUN mkdir -p /build/source \
    && git archive --format=tar HEAD | tar -x -C /build/source \
    && (git config --unset-all http.https://github.com/.extraheader || true) \
    && (git remote remove origin || true) \
    && git remote add origin https://github.com/sneivandt/dotfiles.git \
    && git checkout -B main HEAD \
    && (git branch --set-upstream-to=origin/main main || true)
WORKDIR /build/source
ARG DOTFILES_VERSION
RUN version="${DOTFILES_VERSION:-$(git --git-dir=/build/.git describe --tags --abbrev=0 --match 'v[0-9]*')}" \
    && DOTFILES_VERSION="$version" cargo build --release --manifest-path cli/Cargo.toml \
    && strip cli/target/release/dotfiles

FROM ubuntu:24.04
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
ENV SHELL=/bin/zsh

# Install a self-managing dotfiles checkout. Keep sanitized Git metadata so
# update and sparse-checkout tasks can operate inside the image.
COPY --from=builder --chown=sneivandt:sneivandt /build/source/ /home/sneivandt/dotfiles/
COPY --from=builder --chown=sneivandt:sneivandt /build/.git /home/sneivandt/dotfiles/.git
COPY --from=builder --chown=sneivandt:sneivandt /build/source/cli/target/release/dotfiles /home/sneivandt/dotfiles/bin/dotfiles
USER sneivandt
RUN /home/sneivandt/dotfiles/bin/dotfiles --root /home/sneivandt/dotfiles -p "$PROFILE" install
CMD ["/usr/bin/zsh"]
