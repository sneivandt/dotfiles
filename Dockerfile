# syntax=docker/dockerfile:1
FROM ubuntu:latest AS builder

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
        pkg-config

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build
COPY cli/ cli/
COPY .git .git
RUN cargo build --release --manifest-path cli/Cargo.toml \
    && strip cli/target/release/dotfiles

FROM ubuntu:latest

LABEL org.opencontainers.image.title="dotfiles" \
      org.opencontainers.image.description="Cross-platform dotfiles for Linux/Arch/Windows" \
      org.opencontainers.image.authors="Stuart Neivandt" \
      org.opencontainers.image.url="https://github.com/sneivandt/dotfiles" \
      org.opencontainers.image.source="https://github.com/sneivandt/dotfiles" \
      org.opencontainers.image.documentation="https://github.com/sneivandt/dotfiles/blob/master/README.md" \
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
CMD ["/usr/bin/zsh"]

# Install dotfiles
COPY --chown=sneivandt:sneivandt .git /home/sneivandt/dotfiles/.git
COPY --chown=sneivandt:sneivandt conf /home/sneivandt/dotfiles/conf
COPY --chown=sneivandt:sneivandt symlinks /home/sneivandt/dotfiles/symlinks
COPY --chown=sneivandt:sneivandt hooks /home/sneivandt/dotfiles/hooks
COPY --from=builder /build/cli/target/release/dotfiles /home/sneivandt/dotfiles/bin/dotfiles
USER sneivandt
RUN /home/sneivandt/dotfiles/bin/dotfiles --root /home/sneivandt/dotfiles -p base install \
    && rm -rf /home/sneivandt/dotfiles/.git
