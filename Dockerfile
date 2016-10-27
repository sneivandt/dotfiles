FROM debian:jessie
MAINTAINER sneivandt

# Link sh to bash
RUN rm /bin/sh && ln -s /bin/bash /bin/sh

# Install packages
ENV DEBIAN_FRONTEND noninteractive
RUN apt-get -qq update
RUN apt-get -qqy install git locales tmux vim zsh

# Locale
RUN echo -e "en_US.UTF-8 UTF-8" >> /etc/locale.gen && /usr/sbin/locale-gen

# Install dotfiles
COPY . /root/.dotfiles
RUN /root/.dotfiles/dotfiles.sh install --root

# Use ZSH
RUN chsh -s /usr/bin/zsh
ENTRYPOINT ["/usr/bin/zsh"]
