#!/bin/sh

# User commands must be available to capability checks and session startup.
# shellcheck disable=SC1090
[ -r ~/.config/shell/path.sh ] && . ~/.config/shell/path.sh

# gpg
if tty -s; then
  GPG_TTY="$(tty)"
  export GPG_TTY
fi

# xdg
export XDG_CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}"
export XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
export XDG_DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
if [ "$XDG_CONFIG_HOME" != "$HOME/.config" ]; then
  printf 'dotfiles: custom XDG_CONFIG_HOME is unsupported; managed settings remain under %s\n' \
    "$HOME/.config" >&2
fi

# editor
if command -v nvim >/dev/null 2>&1; then
  _default_editor=nvim
else
  _default_editor=vim
fi
export EDITOR="${EDITOR:-$_default_editor}"
export VISUAL="${VISUAL:-$EDITOR}"
unset _default_editor

# golang
export GOPATH="${GOPATH:-$HOME/src/go}"

# locale
# Preserve the session locale and message-language preference. Default only LANG.
export LANG="${LANG:-en_US.UTF-8}"

# less
# Hardcoded SGR sequences rather than tput: querying terminfo cost one
# subprocess per capability on every login shell, and less only needs the
# handful of attributes below. ssubm/rsubm/ssupm/rsupm are dropped because
# they are not standard terminfo capabilities and always resolved to empty.
if [ -n "$TERM" ] && [ "$TERM" != dumb ]; then
  _esc=$(printf '\033')

  LESS_TERMCAP_mb="${_esc}[1;32m"  # blink  -> bold green
  LESS_TERMCAP_md="${_esc}[1;36m"  # bold   -> bold cyan
  LESS_TERMCAP_me="${_esc}[0m"     # reset
  LESS_TERMCAP_so="${_esc}[1;33;44m" # standout -> bold yellow on blue
  LESS_TERMCAP_se="${_esc}[0m"     # standout end
  LESS_TERMCAP_us="${_esc}[4;1;37m" # underline -> bold underlined white
  LESS_TERMCAP_ue="${_esc}[0m"     # underline end
  LESS_TERMCAP_mr="${_esc}[7m"     # reverse
  LESS_TERMCAP_mh="${_esc}[2m"     # dim

  unset _esc

  export LESS_TERMCAP_mb
  export LESS_TERMCAP_md
  export LESS_TERMCAP_me
  export LESS_TERMCAP_so
  export LESS_TERMCAP_se
  export LESS_TERMCAP_us
  export LESS_TERMCAP_ue
  export LESS_TERMCAP_mr
  export LESS_TERMCAP_mh
fi

LESSHISTFILE=/dev/null
export LESS="${LESS--imRj8}"
export LESSHISTFILE

# mail
unset MAILCHECK

# man
if [ -n "${MANPAGER:-}" ]; then
  export MANPAGER
elif command -v bat >/dev/null 2>&1 && command -v col >/dev/null 2>&1; then
  # MANPAGER is executed through shell; quotes are part of the command string
  # shellcheck disable=SC2089,SC2090
  export MANPAGER="sh -c 'col -bx | bat -l man -p'"
else
  # shellcheck disable=SC2090
  export MANPAGER="less"
fi

# readline
if [ "${INPUTRC+x}" != x ]; then
  if [ -r "$XDG_CONFIG_HOME/readline/inputrc" ]; then
    export INPUTRC="$XDG_CONFIG_HOME/readline/inputrc"
  elif [ -r "$HOME/.config/readline/inputrc" ]; then
    export INPUTRC="$HOME/.config/readline/inputrc"
  fi
fi

# terminfo
export TERMINFO="${TERMINFO:-$XDG_DATA_HOME/terminfo}"
export TERMINFO_DIRS="${TERMINFO_DIRS:-$XDG_DATA_HOME/terminfo:}"

# wget
if [ "${WGETRC+x}" != x ]; then
  if [ -r "$XDG_CONFIG_HOME/wgetrc" ]; then
    export WGETRC="$XDG_CONFIG_HOME/wgetrc"
  elif [ -r "$HOME/.config/wgetrc" ]; then
    export WGETRC="$HOME/.config/wgetrc"
  fi
fi

# fzf
export FZF_CTRL_R_OPTS="${FZF_CTRL_R_OPTS---no-preview}"
if command -v fd >/dev/null 2>&1; then
  export FZF_DEFAULT_COMMAND="${FZF_DEFAULT_COMMAND-fd --type f --strip-cwd-prefix --hidden --follow --exclude .git}"
  export FZF_CTRL_T_COMMAND="${FZF_CTRL_T_COMMAND-$FZF_DEFAULT_COMMAND}"
  export FZF_ALT_C_COMMAND="${FZF_ALT_C_COMMAND-fd --type d --strip-cwd-prefix --hidden --follow --exclude .git}"
  export FZF_DEFAULT_OPTS="${FZF_DEFAULT_OPTS---height 40% --layout=reverse --border}"

  # Preview one directory level and cap the output for large directories.
  # FZF parses the quoted preview command from this options string itself.
  if [ "${FZF_ALT_C_OPTS+x}" != x ]; then
    # shellcheck disable=SC2089,SC2090
    if command -v eza >/dev/null 2>&1; then
      FZF_ALT_C_OPTS="--preview 'eza --all --group-directories-first --color=always -- {} | head -80'"
    else
      FZF_ALT_C_OPTS="--preview 'ls -A -- {} | head -80'"
    fi
  fi
  # shellcheck disable=SC2090
  export FZF_ALT_C_OPTS

  if command -v bat >/dev/null 2>&1; then
    if [ "${FZF_CTRL_T_OPTS+x}" != x ]; then
      # shellcheck disable=SC2089
      FZF_CTRL_T_OPTS="--preview 'bat --style=numbers --color=always --line-range :500 -- {}'"
    fi
    # shellcheck disable=SC2090
    export FZF_CTRL_T_OPTS
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
# Prefer the lazy loader: it installs stub functions and only sources the real
# script (a ~200-500 ms Python startup) on first use.
if command -v virtualenvwrapper_lazy.sh >/dev/null 2>&1; then
  export WORKON_HOME=~/.venv
  . virtualenvwrapper_lazy.sh
elif command -v virtualenvwrapper.sh >/dev/null 2>&1; then
  export WORKON_HOME=~/.venv
  . virtualenvwrapper.sh
fi
