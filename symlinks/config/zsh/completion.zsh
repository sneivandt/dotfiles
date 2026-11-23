#!/usr/bin/env zsh

fpath=(~/.config/zsh/completions $fpath)

autoload -Uz compinit

# Limit full completion scans to once per day.
typeset -g ZSH_COMPDUMP="${ZSH_COMPDUMP:-${XDG_CACHE_HOME:-$HOME/.cache}/zsh/zcompdump-${ZSH_VERSION}}"

# Ensure cache directory exists (extract directory from ZSH_COMPDUMP path)
[[ -d ${ZSH_COMPDUMP:h} ]] || mkdir -p "${ZSH_COMPDUMP:h}"

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

_dotfiles_complete() {
  # Generic candidates quote the leading tilde. Let Zsh complete home paths.
  if [[ $PREFIX == '~/'* && -z $QIPREFIX ]]; then
    _files
    return
  fi
  if (( ! $+functions[_clap_dynamic_completer_dotfiles] )); then
    [[ -r ~/.config/zsh/completions/_dotfiles ]] || return 1
    source ~/.config/zsh/completions/_dotfiles
    if [[ ${_comps[dotfiles]:-} == _clap_dynamic_completer_dotfiles ]]; then
      compdef _dotfiles_complete dotfiles
    fi
  fi
  if (( $+functions[_clap_dynamic_completer_dotfiles] )); then
    _clap_dynamic_completer_dotfiles "$@"
  fi
}

# Register the lazy wrapper without forking dotfiles during shell startup.
if [[ -r ~/.config/zsh/completions/_dotfiles ]] && command -v dotfiles >/dev/null 2>&1; then
  compdef _dotfiles_complete dotfiles
fi

setopt always_to_end
setopt auto_menu
setopt complete_in_word
unsetopt flowcontrol
unsetopt menu_complete

zstyle ':completion:*' group-name ''
zstyle ':completion:*' list-colors ${(s.:.)LS_COLORS:-}
zstyle ':completion:*' matcher-list 'm:{a-zA-Z}={A-Za-z}' 'r:|[._-]=* r:|=*' 'l:|=* r:|=*'
zstyle ':completion:*' menu select=2
zstyle ':completion:*' rehash true
zstyle ':completion:*' use-cache on
zstyle ':completion:*' cache-path "${XDG_CACHE_HOME:-$HOME/.cache}/zsh"
zstyle ':completion:*' verbose yes
zstyle ':completion:*:*:kill:*' menu yes select
zstyle ':completion:*:*:kill:*:processes' list-colors "=(#b) #([0-9]#)*=29=31"
zstyle ':completion:*:*:killall:*' menu yes select
# Only try spelling corrections on a repeated completion of the same input.
zstyle -e ':completion:*' completer '
  reply=(_expand _complete _ignored)
  if [[ ${_DOTFILES_COMPLETION_LAST_TRY-} == "$HISTNO:$BUFFER:$CURSOR" ]]; then
    reply+=(_approximate)
  fi
  _DOTFILES_COMPLETION_LAST_TRY="$HISTNO:$BUFFER:$CURSOR"
'
zstyle ':completion:*:kill:*' force-list always
zstyle ':completion:*:killall:*' force-list always
zstyle ':completion:*:manuals' separate-sections true
zstyle ':completion:*:processes' command 'ps -u "$USER" -o pid,tty,time,args'

# use /etc/hosts and known_hosts for hostname completion
typeset -g _ZSH_HOSTS_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/zsh/hosts.cache"
typeset -gaU hosts

() {
  # Track readable inputs so removal invalidates the cache as well as edits.
  local inputs='# Inputs '
  local newer=false
  local source_file cached_inputs line field tmp_hosts_cache host
  local -a known_hosts etc_hosts ssh_config

  for source_file in /etc/hosts /etc/ssh/ssh_known_hosts ~/.ssh/known_hosts ~/.ssh/config; do
    if [[ -r "$source_file" ]]; then
      inputs+=1
      if [[ "$source_file" -nt "$_ZSH_HOSTS_CACHE" ]]; then
        newer=true
      fi
    else
      inputs+=0
    fi
  done

  if [[ $newer == false && -f "$_ZSH_HOSTS_CACHE" &&
        -n ${_ZSH_HOSTS_CACHE}(#qNmh-24) ]] &&
     IFS= read -r cached_inputs < "$_ZSH_HOSTS_CACHE" &&
     [[ $cached_inputs == "$inputs" ]]; then
    source "$_ZSH_HOSTS_CACHE"
  else
    known_hosts=()
    for source_file in /etc/ssh/ssh_known_hosts ~/.ssh/known_hosts; do
      [[ -r "$source_file" ]] || continue
      for line in "${(@f)$(<"$source_file")}"; do
        field=${line%%[[:blank:]]*}
        [[ -z $field || $field == [\|\#]* ]] && continue
        known_hosts+=("${(@s:,:)field}")
      done
    done
    [ -r /etc/hosts ] && : ${(A)etc_hosts:=${(s: :)${(ps:\t:)${${(f)~~"$(</etc/hosts)"}%%\#*}##[:blank:]#[^[:blank:]]#}}} || etc_hosts=()
    ssh_config=()
    if [[ -r ~/.ssh/config ]] && command -v awk >/dev/null 2>&1; then
      ssh_config=("${(@f)$(awk '
        {
          sub(/#.*/, "")
          sub(/^[[:space:]]+/, "")
          if (tolower($0) ~ /^host([[:space:]]+|=)/) {
            sub(/^[^[:space:]=]+[[:space:]]*=*[[:space:]]*/, "")
            n = split($0, parts, /[[:space:]]+/)
            for (i = 1; i <= n; i++) {
              if (parts[i] != "" && parts[i] !~ /[*?!]/) print parts[i]
            }
          }
        }
      ' ~/.ssh/config)}")
      ssh_config=(${ssh_config:#})
    fi
    hosts=(
      "${known_hosts[@]}"
      "${etc_hosts[@]}"
      "${ssh_config[@]}"
      "${HOST:-}"
      localhost
    )
    hosts=(${hosts:#})

    if mkdir -p "${_ZSH_HOSTS_CACHE:h}" 2>/dev/null &&
       tmp_hosts_cache="$(mktemp "${_ZSH_HOSTS_CACHE}.XXXXXX" 2>/dev/null)"; then
      {
        print -r -- "$inputs"
        print -r -- "typeset -gaU hosts=("
        for host in "${hosts[@]}"; do
          print -r -- "  ${(qqq)host}"
        done
        print -r -- ")"
      } > "$tmp_hosts_cache" && mv "$tmp_hosts_cache" "$_ZSH_HOSTS_CACHE" || rm -f "$tmp_hosts_cache"
    fi
  fi
}
typeset -gaU hosts
zstyle ':completion:*:hosts' hosts $hosts
