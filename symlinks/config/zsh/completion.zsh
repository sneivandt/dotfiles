#!/usr/bin/env zsh

fpath=(~/.config/zsh/completions $fpath)

autoload -Uz compinit

# Limit full completion scans to once per day.
typeset -g ZSH_COMPDUMP="${ZSH_COMPDUMP:-${XDG_CACHE_HOME:-$HOME/.cache}/zsh/zcompdump-${ZSH_VERSION}}"

# Ensure cache directory exists (extract directory from ZSH_COMPDUMP path)
mkdir -p "${ZSH_COMPDUMP:h}"

# compinit leaves an unchanged dump's mtime alone, so track the last scan separately.
if [[ -f "$ZSH_COMPDUMP" && -n ${ZSH_COMPDUMP}.checked(#qNmh-24) ]]; then
  compinit -C -d "$ZSH_COMPDUMP" || return
else
  compinit -d "$ZSH_COMPDUMP" || return
  touch "${ZSH_COMPDUMP}.checked"
  # Compile zsh compdump for faster loading
  if [[ -f "$ZSH_COMPDUMP" && ( ! -f "${ZSH_COMPDUMP}.zwc" || "${ZSH_COMPDUMP}" -nt "${ZSH_COMPDUMP}.zwc" ) ]]; then
    zcompile "$ZSH_COMPDUMP"
  fi
fi

# The generated dynamic registration is a source script, not a #compdef file.
if [[ -r ~/.config/zsh/completions/_dotfiles ]] && command -v dotfiles >/dev/null 2>&1; then
  source ~/.config/zsh/completions/_dotfiles
fi

setopt always_to_end
setopt auto_menu
setopt complete_in_word
unsetopt flowcontrol
unsetopt menu_complete

zstyle ':completion:*' group-name ''
zstyle ':completion:*' list-colors ${(s.:.)LS_COLORS}
zstyle ':completion:*' matcher-list 'm:{a-zA-Z}={A-Za-z}' 'r:|[._-]=* r:|=*' 'l:|=* r:|=*'
zstyle ':completion:*' menu select=2
zstyle ':completion:*' rehash true
zstyle ':completion:*' use-cache on
zstyle ':completion:*' cache-path "${XDG_CACHE_HOME:-$HOME/.cache}/zsh"
zstyle ':completion:*' verbose yes
zstyle ':completion:*:*:kill:*' menu yes select
zstyle ':completion:*:*:kill:*:processes' list-colors "=(#b) #([0-9]#)*=29=31"
zstyle ':completion:*:*:killall:*' menu yes select
zstyle ':completion:*::::' completer _expand _complete _ignored _approximate
zstyle ':completion:*:kill:*' force-list always
zstyle ':completion:*:killall:*' force-list always
zstyle ':completion:*:manuals' separate-sections true
zstyle ':completion:*:processes' command 'ps -au$USER'

# use /etc/hosts and known_hosts for hostname completion
typeset -g _ZSH_HOSTS_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/zsh/hosts.cache"

if [[ -f "$_ZSH_HOSTS_CACHE" && -n ${_ZSH_HOSTS_CACHE}(#qNmh-24) ]]; then
  source "$_ZSH_HOSTS_CACHE"
else
  [ -r /etc/ssh/ssh_known_hosts ] && _global_ssh_hosts=(${${${${(f)"$(</etc/ssh/ssh_known_hosts)"}:#[\|]*}%%\ *}%%,*}) || _global_ssh_hosts=()
  [ -r ~/.ssh/known_hosts ] && _ssh_hosts=(${${${${(f)"$(<~/.ssh/known_hosts)"}:#[\|]*}%%\ *}%%,*}) || _ssh_hosts=()
  [ -r /etc/hosts ] && : ${(A)_etc_hosts:=${(s: :)${(ps:\t:)${${(f)~~"$(</etc/hosts)"}%%\#*}##[:blank:]#[^[:blank:]]#}}} || _etc_hosts=()
  [ -r ~/.ssh/config ] && _ssh_config=($(sed -ne 's/Host[=\t ]//p' ~/.ssh/config)) || _ssh_config=()
  hosts=(
    "$_global_ssh_hosts[@]"
    "$_ssh_hosts[@]"
    "$_etc_hosts[@]"
    "$_ssh_config[@]"
    "$HOST"
    localhost
  )

  if mkdir -p "${_ZSH_HOSTS_CACHE:h}"; then
    tmp_hosts_cache="$(mktemp "${_ZSH_HOSTS_CACHE}.XXXXXX")" || return 1
    {
      print -r -- "typeset -ga hosts=("
      for host in "${hosts[@]}"; do
        print -r -- "  ${(qqq)host}"
      done
      print -r -- ")"
    } > "$tmp_hosts_cache"
    mv "$tmp_hosts_cache" "$_ZSH_HOSTS_CACHE"
  fi
fi
zstyle ':completion:*:hosts' hosts $hosts
