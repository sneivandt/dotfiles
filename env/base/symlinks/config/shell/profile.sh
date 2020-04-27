#!/bin/sh

# editor
export EDITOR=vim

# golang
export GOPATH=~/src/go

# locale
export LC_ALL=en_US.UTF-8
export LANG=en_US.UTF-8
export LANGUAGE=en_US.UTF-8

# less
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
export MANPAGER="less -imRj8X"

# _shellcheck
export SHELLCHECK_OPTS="-e SC1090 -e SC1091"

# wget
export WGETRC=~/.config/wgetrc

# virtualenvwrapper
if [ -n "$(command -vp virtualenvwrapper.sh)" ]
then
  export WORKON_HOME=~/.venv
  . virtualenvwrapper.sh
fi

# xdg
export XDG_CACHE_HOME="$HOME"/.cache
export XDG_CONFIG_HOME="$HOME"/.config

# wsl
if [ -n "$(command -vp wslpath)" ]
then
  WINDRIVE=$(wslpath -a -u "$(cmd.exe /c "echo %SYSTEMDRIVE%\\" 2>/dev/null)" | sed s"/..$//")
  export WINDRIVE
  WINHOME=$(wslpath -a -u "$(cmd.exe /c "echo %USERPROFILE%\\" 2>/dev/null)" | sed s"/..$//")
  export WINHOME
fi
