FROM ubuntu:jammy

LABEL maintainer="sneivandt"

ENV DEBIAN_FRONTEND=noninteractive

# Install packages
RUN apt-get update \
    && apt-get install --no-install-recommends --no-install-suggests -y \
        ca-certificates \
        curl \
        exuberant-ctags \
        git \
        locales \
        openssh-client \
        shellcheck \
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
WORKDIR /home/sneivandt
ENV SHELL=/bin/zsh
CMD ["/usr/bin/zsh"]

# Install dotfiles
COPY --chown=sneivandt:sneivandt .git /home/sneivandt/dotfiles/.git
COPY --chown=sneivandt:sneivandt conf /home/sneivandt/dotfiles/conf
COPY --chown=sneivandt:sneivandt src /home/sneivandt/dotfiles/src
COPY --chown=sneivandt:sneivandt symlinks /home/sneivandt/dotfiles/symlinks
COPY --chown=sneivandt:sneivandt test /home/sneivandt/dotfiles/test
COPY --chown=sneivandt:sneivandt dotfiles.sh /home/sneivandt/dotfiles/dotfiles.sh
USER sneivandt
RUN /home/sneivandt/dotfiles/dotfiles.sh --install --profile base \
    && rm -rf /home/sneivandt/dotfiles/.git
