FROM debian:stretch

LABEL maintainer="sneivandt"

ENV DEBIAN_FRONTEND noninteractive

# Install packages
RUN apt-get update \
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
        zsh \
    && rm -rf /var/lib/apt/lists/*

# Configure locale
RUN echo "en_US.UTF-8 UTF-8" > /etc/locale.gen \
    && locale-gen en_US.UTF-8 \
    && dpkg-reconfigure locales \
    && /usr/sbin/update-locale LANG=en_US.UTF-8

# Add user
RUN useradd -ms /usr/bin/zsh dot
ENV SHELL /usr/bin/zsh

# Install dotfiles
COPY . /home/dot/dotfiles
RUN chown -R dot:dot /home/dot
USER dot
RUN /home/dot/dotfiles/dotfiles.sh install

# Entry
WORKDIR /home/dot
ENTRYPOINT /usr/bin/zsh
