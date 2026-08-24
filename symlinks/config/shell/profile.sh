#!/bin/sh

# gpg
if tty -s; then
  GPG_TTY="$(tty)"
  export GPG_TTY
fi

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
# Set LANG (and LANGUAGE) rather than LC_ALL; LC_ALL overrides every locale
# category and prevents per-category customization.
export LANG=en_US.UTF-8
export LANGUAGE=en_US.UTF-8

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
export LESS=-imRj8X
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
  export FZF_CTRL_R_OPTS="--no-preview"

  if command -v bat >/dev/null 2>&1; then
    export FZF_CTRL_T_OPTS="--preview 'bat --style=numbers --color=always --line-range :500 {}'"
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

# wsl
# Each cmd.exe round trip costs a few hundred milliseconds, so the resolved
# paths are cached and only recomputed when the cache is missing. Delete
# "$XDG_CACHE_HOME/wsl-env" if the Windows drive or profile path ever changes.
if command -v wslpath >/dev/null 2>&1; then
  _wsl_cache="${XDG_CACHE_HOME:-$HOME/.cache}/wsl-env"

  if [ ! -s "$_wsl_cache" ]; then
    _windrive=$(wslpath -a -u "$(cmd.exe /c "echo %SYSTEMDRIVE%\\" 2>/dev/null)" | sed s"/..$//")
    _winhome=$(wslpath -a -u "$(cmd.exe /c "echo %USERPROFILE%\\" 2>/dev/null)" | sed s"/..$//")
    if [ -n "$_windrive" ] && [ -n "$_winhome" ]; then
      mkdir -p "${XDG_CACHE_HOME:-$HOME/.cache}"
      printf '%s\n%s\n' "$_windrive" "$_winhome" > "$_wsl_cache"
    fi
    unset _windrive _winhome
  fi

  # Read as plain lines rather than sourcing, so paths containing spaces or
  # shell metacharacters are handled literally.
  if [ -s "$_wsl_cache" ]; then
    { IFS= read -r WINDRIVE; IFS= read -r WINHOME; } < "$_wsl_cache"
    export WINDRIVE
    export WINHOME
  fi

  unset _wsl_cache
fi
