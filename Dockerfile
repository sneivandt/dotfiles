FROM debian
MAINTAINER sneivandt

# Link sh to bash
RUN rm /bin/sh && ln -s /bin/bash /bin/sh

# Install packages
ENV DEBIAN_FRONTEND noninteractive
RUN apt-get -qq update
RUN apt-get -qqy install git locales openssh-server tmux vim zsh

# SSH server
RUN mkdir /var/run/sshd
RUN echo 'root:root' | chpasswd
RUN sed -i 's/PermitRootLogin without-password/PermitRootLogin yes/' /etc/ssh/sshd_config

# Locale
RUN echo -e "en_US.UTF-8 UTF-8" >> /etc/locale.gen && /usr/sbin/locale-gen

# Install dotfiles
COPY . /root/.dotfiles
RUN /root/.dotfiles/dot.sh install --root

# Use zsh
RUN chsh -s /usr/bin/zsh

# sshd
EXPOSE 22
CMD ["/usr/sbin/sshd", "-D"]
