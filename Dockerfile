FROM debian:buster

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
        ranger \
        tmux \
        vim \
        wget \
        zip \
        zsh \
    && rm -rf /var/lib/apt/lists/*

# Configure locale
RUN echo "en_US.UTF-8 UTF-8" > /etc/locale.gen \
    && locale-gen

# Add user
RUN useradd -ms /bin/zsh sneivandt
ENV SHELL /bin/zsh

# Install dotfiles
COPY . /home/sneivandt/dotfiles
RUN chown -R sneivandt:sneivandt /home/sneivandt
USER sneivandt
RUN /home/sneivandt/dotfiles/dotfiles.sh --install

# Entry
WORKDIR /home/sneivandt
ENTRYPOINT /usr/bin/zsh
