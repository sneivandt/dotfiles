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
# Performance: Only call tput once per capability, cache results
if [ -n "$TERM" ] && command -v tput >/dev/null 2>&1; then
  _tput_bold=$(tput bold)
  _tput_sgr0=$(tput sgr0)
  _tput_setaf_2=$(tput setaf 2)
  _tput_setaf_6=$(tput setaf 6)
  _tput_setaf_3=$(tput setaf 3)
  _tput_setab_4=$(tput setab 4)
  _tput_setaf_7=$(tput setaf 7)
  _tput_smul=$(tput smul)
  _tput_rmul=$(tput rmul)
  _tput_rmso=$(tput rmso)
  _tput_rev=$(tput rev)
  _tput_dim=$(tput dim)

  LESS_TERMCAP_mb="${_tput_bold}${_tput_setaf_2}"
  LESS_TERMCAP_md="${_tput_bold}${_tput_setaf_6}"
  LESS_TERMCAP_me="${_tput_sgr0}"
  LESS_TERMCAP_so="${_tput_bold}${_tput_setaf_3}${_tput_setab_4}"
  LESS_TERMCAP_se="${_tput_rmso}${_tput_sgr0}"
  LESS_TERMCAP_us="${_tput_smul}${_tput_bold}${_tput_setaf_7}"
  LESS_TERMCAP_ue="${_tput_rmul}${_tput_sgr0}"
  LESS_TERMCAP_mr="${_tput_rev}"
  LESS_TERMCAP_mh="${_tput_dim}"

  # Note: ssubm/rsubm/ssupm/rsupm are not standard terminfo capabilities
  # Only define if tput doesn't fail
  _tput_ssubm=$(tput ssubm 2>/dev/null) || true
  _tput_rsubm=$(tput rsubm 2>/dev/null) || true
  _tput_ssupm=$(tput ssupm 2>/dev/null) || true
  _tput_rsupm=$(tput rsupm 2>/dev/null) || true

  LESS_TERMCAP_ZN="${_tput_ssubm}"
  LESS_TERMCAP_ZV="${_tput_rsubm}"
  LESS_TERMCAP_ZO="${_tput_ssupm}"
  LESS_TERMCAP_ZW="${_tput_rsupm}"

  # Clean up temporary variables
  unset _tput_bold _tput_sgr0 _tput_setaf_2 _tput_setaf_6 _tput_setaf_3
  unset _tput_setab_4 _tput_setaf_7 _tput_smul _tput_rmul _tput_rmso
  unset _tput_rev _tput_dim _tput_ssubm _tput_rsubm _tput_ssupm _tput_rsupm
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

# eza/exa colors
# Configure eza colors (modern ls replacement) to be less extreme
# Reset most permission bits to plain colors without bold/bright attributes
export EZA_COLORS="\
ur=0:uw=0:ux=0:ue=0:\
gr=0:gw=0:gx=0:\
tr=0:tw=0:tx=0:\
su=0:sf=0:xa=0:\
uu=0:un=0:\
gu=0:gn=0:\
da=0:\
sn=0:sb=0:\
nb=0:\
nk=0:nm=0:ng=0:nt=0"

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
