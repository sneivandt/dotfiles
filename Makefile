DOTFILES := $(shell pwd)
PREFIX   := "\033[1;34m::\033[0m\033[1m "
SUFIX    := " ...\033[0m"

all: install

help:
	@echo "install:   Update git submodules, create symlinks and install vim plugins"
	@echo "uninstall: Remove symlinks"

install: _submodules _symlinks _vim

uninstall: _symlinks-remove

_submodules:
	@echo -e $(PREFIX)"Updating git submodules"$(SUFIX)
	@git submodule init
	@git submodule update

_vim:
	@echo -e $(PREFIX)"Installing vim plugins"$(SUFIX)
	@vim +PluginInstall +qall

_symlinks:
	@echo -e $(PREFIX)"Creating symlinks"$(SUFIX)
	@mkdir -p ~/.ssh
	@ln -snfv $(DOTFILES)/aliases      ~/.aliases
	@ln -snfv $(DOTFILES)/bash_profile ~/.bash_profile
	@ln -snfv $(DOTFILES)/bashrc       ~/.bashrc
	@ln -snfv $(DOTFILES)/gitconfig    ~/.gitconfig
	@ln -snfv $(DOTFILES)/gitignore    ~/.gitignore
	@ln -snfv $(DOTFILES)/gtkrc-2.0    ~/.gtkrc-2.0
	@ln -snfv $(DOTFILES)/i3           ~/.i3
	@ln -snfv $(DOTFILES)/profile      ~/.profile
	@ln -snfv $(DOTFILES)/ssh/config   ~/.ssh/config
	@ln -snfv $(DOTFILES)/tmux.conf    ~/.tmux.conf
	@ln -snfv $(DOTFILES)/vim          ~/.vim
	@ln -snfv $(DOTFILES)/wgetrc       ~/.wgetrc
	@ln -snfv $(DOTFILES)/xinitrc      ~/.xinitrc
	@ln -snfv $(DOTFILES)/Xresources   ~/.Xresources
	@ln -snfv $(DOTFILES)/zsh          ~/.zsh
	@ln -snfv $(DOTFILES)/zshenv       ~/.zshenv
	@ln -snfv $(DOTFILES)/zshrc        ~/.zshrc
	@chmod 600 ~/.ssh/config

_symlinks-remove:
	@echo -e $(PREFIX)"Removing symlinks"$(SUFIX)
	@rm -v ~/.aliases
	@rm -v ~/.bash_profile
	@rm -v ~/.bashrc
	@rm -v ~/.gitconfig
	@rm -v ~/.gitignore
	@rm -v ~/.gtkrc-2.0
	@rm -v ~/.i3
	@rm -v ~/.profile
	@rm -v ~/.ssh/config
	@rm -v ~/.tmux.conf
	@rm -v ~/.vim
	@rm -v ~/.wgetrc
	@rm -v ~/.xinitrc
	@rm -v ~/.Xresources
	@rm -v ~/.zsh
	@rm -v ~/.zshenv
	@rm -v ~/.zshrc
