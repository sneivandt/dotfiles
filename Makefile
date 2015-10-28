SHELL := /bin/bash

FILES := `cat list`

PREFIX := "\033[1;34m::\033[0m\033[1m "
SUFFIX := " ...\033[0m"

define root_check
	if [[ $$EUID -eq 0 && -z $$allow_root ]]; then echo "ABORTING: Do not run as root" && exit 1; fi
endef

all: install

help:
	@echo "install:   Install git submodules, install vim plugins and create symlinks"
	@echo "uninstall: Remove symlinks"

install:
	@$(call root_check)
	@echo -e $(PREFIX)"Installing git submodules"$(SUFFIX)
	@git submodule update --init
	@echo -e $(PREFIX)"Creating symlinks"$(SUFFIX)
	@for link in $(FILES); do \
		if [[ (-z `cat .listignore 2>/dev/null | grep -Fx $$link`) && (`readlink -f $(shell pwd)/files/$$link` != `readlink -f ~/.$$link`) ]]; then \
			if [[ $$link == *"/"* ]]; then \
				mkdir -pv ~/.`echo $$link | rev | cut -d/ -f2- | rev`; \
			fi; \
			ln -snvf $(shell pwd)/files/$$link ~/.$$link; \
		fi \
	done
	@chmod -c 600 ~/.ssh/config 2>/dev/null
	@echo -e $(PREFIX)"Installing vim plugins"$(SUFFIX)
	@vim +PlugUpdate +qall

uninstall:
	@$(call root_check)
	@for link in $(FILES); do \
		if [[ (-z `cat .listignore 2>/dev/null | grep -Fx $$link`) && (`readlink -f $(shell pwd)/files/$$link` == `readlink -f ~/.$$link`) ]]; then \
			rm -vf ~/.$$link; \
		fi \
	done
