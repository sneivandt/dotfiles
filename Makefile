LINKS := \
	bash_profile  \
	bashrc        \
	curlrc        \
	gitattributes \
	gitconfig     \
	gitignore     \
	gtkrc-2.0     \
	i3            \
	shell         \
	ssh/config    \
	tmux.conf     \
	vim           \
	wgetrc        \
	xinitrc       \
	Xresources    \
	zsh           \
	zshenv        \
	zshrc

PREFIX := "\033[1;34m::\033[0m\033[1m "
SUFFIX := " ...\033[0m"

all: install

help:
	@echo "install:   Install git submodules, vim plugins and create symlinks"
	@echo "uninstall: Remove symlinks"

install:
	@echo -e $(PREFIX)"Installing git submodules"$(SUFFIX)
	@git submodule update --init
	@echo -e $(PREFIX)"Installing vim plugins"$(SUFFIX)
	@vim +PlugUpdate +qall
	@echo -e $(PREFIX)"Creating symlinks"$(SUFFIX)
	@mkdir -pv ~/.ssh
	@for link in $(LINKS); do \
		if [[ ! -e ~/.$$link && -z `cat .linkignore 2>/dev/null | grep -Fx $$link` ]]; then \
			ln -snvf $(shell pwd)/$$link ~/.$$link; \
		fi \
	done
	@chmod -c 600 ~/.ssh/config

uninstall:
	@echo -e $(PREFIX)"Removing symlinks"$(SUFFIX)
	@for link in $(LINKS); do \
		if [[ -z `cat .linkignore 2>/dev/null | grep -Fx $$link` ]]; then \
			rm -vf ~/.$$link; \
		fi \
	done
