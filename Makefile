DOTFILES := $(shell pwd)
PREFIX   := "\033[1;34m::\033[0m\033[1m "
SUFFIX   := " ...\033[0m"
LINKS    := \
	aliases       \
	bash_profile  \
	bashrc        \
	functions     \
	gitattributes \
	gitconfig     \
	gitignore     \
	gtkrc-2.0     \
	i3            \
	profile       \
	ssh/config    \
	tmux.conf     \
	vim           \
	wgetrc        \
	xinitrc       \
	Xresources    \
	zsh           \
	zshenv        \
	zshrc

all: install

help:
	@echo "install:   Install git submodules, create symlinks and install vim plugins"
	@echo "uninstall: Remove symlinks"

install: _submodule _symlinks _vim

uninstall: _symlinks-remove

_submodule:
	@echo -e $(PREFIX)"Installing git submodules"$(SUFFIX)
	@git submodule update --init

_symlinks:
	@echo -e $(PREFIX)"Creating symlinks"$(SUFFIX)
	@mkdir -pv ~/.ssh
	@for link in $(LINKS); do \
		if [[ ! -e ~/.$$link ]]; then \
			ln -snvf $(DOTFILES)/$$link ~/.$$link; \
		fi \
	done
	@chmod -c 600 ~/.ssh/config

_vim:
	@echo -e $(PREFIX)"Installing vim plugins"$(SUFFIX)
	@vim +PlugInstall +qall

_symlinks-remove:
	@echo -e $(PREFIX)"Removing symlinks"$(SUFFIX)
	@for link in $(LINKS); do \
		rm -vf ~/.$$link; \
	done
