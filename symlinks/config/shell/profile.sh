#!/bin/sh

# gpg
GPG_TTY="$(tty)"
export GPG_TTY

# xdg
export XDG_CACHE_HOME="$HOME"/.cache
export XDG_CONFIG_HOME="$HOME"/.config
export XDG_DATA_HOME="$HOME"/.local/share

# editor
if command -v nvim >/dev/null 2>&1; then
  export EDITOR=nvim
  export VISUAL=nvim
else
  export EDITOR=vim
  export VISUAL=vim
fi

# golang
export GOPATH=~/src/go

# locale
export LC_ALL=en_US.UTF-8
export LANG=en_US.UTF-8
export LANGUAGE=en_US.UTF-8

# less
if [ -n "$TERM" ] && command -v tput >/dev/null 2>&1; then
  LESS_TERMCAP_mb=$(tput bold; tput setaf 2) # green
  LESS_TERMCAP_md=$(tput bold; tput setaf 6) # cyan
  LESS_TERMCAP_me=$(tput sgr0)
  LESS_TERMCAP_so=$(tput bold; tput setaf 3; tput setab 4) # yellow on blue
  LESS_TERMCAP_se=$(tput rmso; tput sgr0)
  LESS_TERMCAP_us=$(tput smul; tput bold; tput setaf 7) # white
  LESS_TERMCAP_ue=$(tput rmul; tput sgr0)
  LESS_TERMCAP_mr=$(tput rev)
  LESS_TERMCAP_mh=$(tput dim)
  LESS_TERMCAP_ZN=$(tput ssubm)
  LESS_TERMCAP_ZV=$(tput rsubm)
  LESS_TERMCAP_ZO=$(tput ssupm)
  LESS_TERMCAP_ZW=$(tput rsupm)
fi
LESSHISTFILE=/dev/null
export LESS=-imRj8X
export LESS_TERMCAP_mb
export LESS_TERMCAP_md
export LESS_TERMCAP_me
export LESS_TERMCAP_so
export LESS_TERMCAP_se
export LESS_TERMCAP_us
export LESS_TERMCAP_ue
export LESS_TERMCAP_mr
export LESS_TERMCAP_mh
export LESS_TERMCAP_ZN
export LESS_TERMCAP_ZV
export LESS_TERMCAP_ZO
export LESS_TERMCAP_ZW
export LESSHISTFILE

# mail
unset MAILCHECK

# man
if command -v bat >/dev/null 2>&1; then
  # MANPAGER is executed through shell; quotes are part of the command string
  # shellcheck disable=SC2089,SC2090
  export MANPAGER="sh -c 'col -bx | bat -l man -p'"
else
  # shellcheck disable=SC2090
  export MANPAGER="less -imRj8X"
fi

# readline
export INPUTRC="$XDG_CONFIG_HOME"/readline/inputrc

# _shellcheck
export SHELLCHECK_OPTS="-e SC1090 -e SC1091"

# terminfo
export TERMINFO="$XDG_DATA_HOME"/terminfo
export TERMINFO_DIRS="$XDG_DATA_HOME"/terminfo:/usr/share/terminfo

# wget
export WGETRC="$XDG_CONFIG_HOME"/wgetrc

# fzf
if command -v fd >/dev/null 2>&1; then
  export FZF_DEFAULT_COMMAND='fd --type f --strip-cwd-prefix --hidden --follow --exclude .git'
  export FZF_CTRL_T_COMMAND="$FZF_DEFAULT_COMMAND"
  export FZF_ALT_C_COMMAND='fd --type d --strip-cwd-prefix --hidden --follow --exclude .git'
  export FZF_DEFAULT_OPTS="--height 40% --layout=reverse --border"

  if command -v bat >/dev/null 2>&1; then
    export FZF_DEFAULT_OPTS="$FZF_DEFAULT_OPTS --preview 'bat --style=numbers --color=always --line-range :500 {}'"
  fi
fi

# bat
export BAT_THEME="ansi"

# eza/exa - minimal colors
export EZA_COLORS="di=34:ex=32"

# virtualenvwrapper
if command -v virtualenvwrapper.sh >/dev/null 2>&1; then
  export WORKON_HOME=~/.venv
  . virtualenvwrapper.sh
fi

# wsl
if command -v wslpath >/dev/null 2>&1; then
  WINDRIVE=$(wslpath -a -u "$(cmd.exe /c "echo %SYSTEMDRIVE%\\" 2>/dev/null)" | sed s"/..$//")
  export WINDRIVE
  WINHOME=$(wslpath -a -u "$(cmd.exe /c "echo %USERPROFILE%\\" 2>/dev/null)" | sed s"/..$//")
  export WINHOME
fi
