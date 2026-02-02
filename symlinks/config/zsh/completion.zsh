#!/usr/bin/env zsh

fpath=(~/.config/zsh/completions $fpath)

autoload -Uz compinit

# Performance: Only regenerate compdump once per day
# This significantly speeds up shell startup
typeset -g ZSH_COMPDUMP="${ZSH_COMPDUMP:-${HOME}/.cache/zsh/zcompdump-${ZSH_VERSION}}"
mkdir -p ~/.cache/zsh

# Check if compdump needs regeneration (once per day)
if [[ -n ${ZSH_COMPDUMP}(#qNmh-20) ]]; then
  # Dump file is recent, use fast mode
  compinit -C -d "$ZSH_COMPDUMP"
else
  # Regenerate dump file
  compinit -d "$ZSH_COMPDUMP"
  # Compile zsh compdump for faster loading
  if [[ ! -f "${ZSH_COMPDUMP}.zwc" || "${ZSH_COMPDUMP}" -nt "${ZSH_COMPDUMP}.zwc" ]]; then
    zcompile "$ZSH_COMPDUMP"
  fi
fi

setopt always_to_end
setopt auto_menu
setopt complete_in_word
setopt completealiases
unsetopt flowcontrol
unsetopt menu_complete

zstyle ':completion:*' group-name ''
zstyle ':completion:*' list-colors ${(s.:.)LS_COLORS}
zstyle ':completion:*' matcher-list 'm:{a-zA-Z}={A-Za-z}' 'r:|[._-]=* r:|=*' 'l:|=* r:|=*'
zstyle ':completion:*' menu select=2
zstyle ':completion:*' rehash true
zstyle ':completion:*' use-cache on
zstyle ':completion:*' cache-path ~/.cache/zsh
zstyle ':completion:*' users $users
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
[ -r /etc/ssh/ssh_known_hosts ] && _global_ssh_hosts=(${${${${(f)"$(</etc/ssh/ssh_known_hosts)"}:#[\|]*}%%\ *}%%,*}) || _ssh_hosts=()
[ -r ~/.ssh/known_hosts ] && _ssh_hosts=(${${${${(f)"$(<~/.ssh/known_hosts)"}:#[\|]*}%%\ *}%%,*}) || _ssh_hosts=()
[ -r /etc/hosts ] && : ${(A)_etc_hosts:=${(s: :)${(ps:\t:)${${(f)~~"$(</etc/hosts)"}%%\#*}##[:blank:]#[^[:blank:]]#}}} || _etc_hosts=()
[ -r ~/.ssh/config ] && _ssh_config=($(cat ~/.ssh/config | sed -ne 's/Host[=\t ]//p')) || _ssh_config=()
hosts=(
  "$_global_ssh_hosts[@]"
  "$_ssh_hosts[@]"
  "$_etc_hosts[@]"
  "$_ssh_config[@]"
  "$HOST"
  localhost
)
zstyle ':completion:*:hosts' hosts $hosts
