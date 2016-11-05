FROM debian:jessie
MAINTAINER sneivandt

# Install packages
ENV DEBIAN_FRONTEND noninteractive
RUN apt-get -qq update
RUN apt-get -qqy install git tmux vim zsh

# Install dotfiles
COPY . /root/.dotfiles
RUN /root/.dotfiles/dotfiles.sh install --root

# Entrypoint
RUN chsh -s /usr/bin/zsh
ENTRYPOINT ["/usr/bin/zsh"]
