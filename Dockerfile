FROM debian:jessie
MAINTAINER sneivandt

# Install packages
ENV DEBIAN_FRONTEND noninteractive
RUN apt-get -qqy update
RUN apt-get -qqy install curl git locales tmux vim wget zsh

# Configure locale
RUN echo "en_US.UTF-8 UTF-8" > /etc/locale.gen
RUN locale-gen en_US.UTF-8
RUN dpkg-reconfigure locales
RUN /usr/sbin/update-locale LANG=en_US.UTF-8
ENV LC_ALL en_US.UTF-8

# Install dotfiles
COPY . /root/.dotfiles
RUN /root/.dotfiles/dotfiles.sh install --root

# Entrypoint
WORKDIR /root
RUN chsh -s /usr/bin/zsh
ENTRYPOINT "/usr/bin/zsh"
