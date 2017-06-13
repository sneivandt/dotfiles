FROM debian:jessie

MAINTAINER sneivandt

ENV DEBIAN_FRONTEND noninteractive

RUN apt-get update \
    && apt-get install --no-install-recommends --no-install-suggests -y \
        ca-certificates \
        curl \
        git \
        locales \
        tmux \
        vim \
        wget \
        zsh \
    && rm -rf /var/lib/apt/lists/*

# Configure locale
RUN echo "en_US.UTF-8 UTF-8" > /etc/locale.gen \
    && locale-gen en_US.UTF-8 \
    && dpkg-reconfigure locales \
    && /usr/sbin/update-locale LANG=en_US.UTF-8

# Install dotfiles
COPY . /root/.dotfiles
RUN /root/.dotfiles/dotfiles.sh install --root

ENTRYPOINT "/usr/bin/zsh"
