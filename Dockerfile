FROM debian
MAINTAINER Stuart Neivandt

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
RUN git clone https://github.com/sneivandt/dotfiles.git /root/.dotfiles
RUN echo -e "atom/config.cson\nconfig/gtk-3.0/settings.ini\neclipse/eclipse-formatter.xml\ni3\ngtk-2.0\nxinitrc\nXresources" >> /root/.dotfiles/.filesignore
RUN /root/.dotfiles/setup.sh install --allow-root

# Use zsh
RUN chsh -s /usr/bin/zsh

# sshd
EXPOSE 22
CMD ["/usr/sbin/sshd", "-D"]
