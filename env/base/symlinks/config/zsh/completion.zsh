#!/usr/bin/env zsh

fpath=(~/.config/zsh/completions $fpath)

autoload -Uz compinit

setopt EXTENDEDGLOB
for dump in $ZSH_COMPDUMP(#qN.m1)
do
  compinit
  if [ -s "$dump" ] && (! -s "$dump.zwc" || "$dump" -nt "$dump.zwc")
  then
    zcompile "$dump"
  fi
done
unsetopt EXTENDEDGLOB
compinit -C -d ~/.cache/zsh/zcompdump-$ZSH_VERSION

setopt always_to_end
setopt auto_menu
setopt complete_in_word
setopt completealiases
unsetopt flowcontrol
unsetopt menu_complete

zstyle ':completion:*' group-name ''
zstyle ':completion:*' list-colors ${(s.:.)LS_COLORS}
zstyle ':completion:*' matcher-list 'm:{a-zA-Z}={A-Za-z}'
zstyle ':completion:*' menu select=2
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
