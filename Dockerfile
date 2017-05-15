FROM debian:jessie
MAINTAINER sneivandt

# Install packages
ENV DEBIAN_FRONTEND noninteractive
RUN apt-get -qqy update
RUN apt-get -qqy install curl git tmux vim wget zsh

# Install dotfiles
COPY . /root/.dotfiles
RUN /root/.dotfiles/dotfiles.sh install --root

# Entrypoint
WORKDIR /root
RUN chsh -s /usr/bin/zsh
ENTRYPOINT "/usr/bin/zsh"
