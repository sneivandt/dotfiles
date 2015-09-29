SHELL := /bin/bash

LINKS := \
	atom/config.cson            \
	bash_profile                \
	bashrc                      \
	config/gtk-3.0/settings.ini \
	curlrc                      \
	gitattributes               \
	gitconfig                   \
	gitignore                   \
	gtkrc-2.0                   \
	i3                          \
	shell                       \
	ssh/config                  \
	tmux.conf                   \
	vim                         \
	wgetrc                      \
	xinitrc                     \
	Xresources                  \
	zsh                         \
	zshenv                      \
	zshrc

PREFIX := "\033[1;34m::\033[0m\033[1m "
SUFFIX := " ...\033[0m"

define root_check
	if [[ $$EUID -eq 0 && -z $$allow_root ]]; then echo "ABORTING: Do not run as root" && exit 1; fi
endef

all: install

help:
	@echo "install:   Install git submodules, vim plugins and create symlinks"
	@echo "uninstall: Remove symlinks"

install:
	@$(call root_check)
	@echo -e $(PREFIX)"Installing git submodules"$(SUFFIX)
	@git submodule update --init
	@echo -e $(PREFIX)"Creating symlinks"$(SUFFIX)
	@mkdir -pv ~/.atom ~/.config/gtk-3.0 ~/.ssh
	@for link in $(LINKS); do \
		if [[ (-z `cat .linkignore 2>/dev/null | grep -Fx $$link`) && (`readlink -f $(shell pwd)/$$link` != `readlink -f ~/.$$link`) ]]; then \
			ln -snvf $(shell pwd)/$$link ~/.$$link; \
		fi \
	done
	@chmod -c 600 ~/.ssh/config
	@echo -e $(PREFIX)"Installing vim plugins"$(SUFFIX)
	@vim +PlugUpdate +qall

uninstall:
	@$(call root_check)
	@for link in $(LINKS); do \
		if [[ -z `cat .linkignore 2>/dev/null | grep -Fx $$link` ]]; then \
			rm -vf ~/.$$link; \
		fi \
	done
