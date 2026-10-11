#!/usr/bin/env zsh

# Keep shell editing independent of EDITOR/VISUAL (which otherwise select viins).
bindkey -e

# Standard navigation uses terminfo, with common normal-mode sequences as fallbacks.
bindkey '\e[H' beginning-of-line
bindkey '\e[F' end-of-line
bindkey '\e[1~' beginning-of-line
bindkey '\e[4~' end-of-line
bindkey '\e[3~' delete-char
if zmodload zsh/terminfo 2>/dev/null; then
  [[ -z ${terminfo[khome]} ]] || bindkey "${terminfo[khome]}" beginning-of-line
  [[ -z ${terminfo[kend]} ]] || bindkey "${terminfo[kend]}" end-of-line
  [[ -z ${terminfo[kdch1]} ]] || bindkey "${terminfo[kdch1]}" delete-char
fi

# Modified keys lack portable terminfo entries. Retain rxvt and CSI conventions.
bindkey '\e[7~' beginning-of-line
bindkey '\e[8~' end-of-line
bindkey '\e[7^' beginning-of-line
bindkey '\e[8^' end-of-line
bindkey '\e[1;5H' beginning-of-line
bindkey '\e[1;5F' end-of-line
bindkey '\e[3^' kill-word
bindkey '\e[3;5~' kill-word
bindkey '\e[1;5D' backward-word
bindkey '\e[1;5C' forward-word
# tmux reserves C-h for pane navigation; ESC DEL works inside and outside tmux.
[[ "$TERM" == (screen*|tmux*) ]] || bindkey '^H' backward-kill-word

# Ctrl + Backspace (ESC DEL).
# The bare ^H bindings above only work outside tmux: tmux claims C-h at the
# root level for vim-tmux-navigator, and Ctrl+Backspace is indistinguishable
# from Ctrl+H on the wire. Terminals we control (see alacritty.toml) are
# configured to emit ESC DEL instead, which reaches the shell either way.
bindkey '\e^?' backward-kill-word

# Magic space history expansion
bindkey ' ' magic-space

# Shift+Tab Reverse menu complete
bindkey '^[[Z' reverse-menu-complete

# Ctrl+r Search history
bindkey '^R' history-incremental-search-backward

# Esc-Esc Toggle "sudo " at the start of line
# (Bound to Esc-Esc rather than ^T because fzf's key-bindings.zsh, sourced
# later, claims ^T for its file widget.)
sudo-command-line()
{
  [[ -z $BUFFER ]] && zle up-history
  local cursor=$CURSOR
  if [[ $BUFFER == sudo\ * ]]; then
    BUFFER="${BUFFER#sudo }"
    CURSOR=$(( cursor > 5 ? cursor - 5 : 0 ))
  else
    BUFFER="sudo $BUFFER"
    CURSOR=$(( cursor + 5 ))
  fi
}
zle -N sudo-command-line
bindkey '\e\e' sudo-command-line

# Search based on what you typed so far
autoload -U up-line-or-beginning-search
autoload -U down-line-or-beginning-search
zle -N up-line-or-beginning-search
zle -N down-line-or-beginning-search
bindkey "^[[A" up-line-or-beginning-search # Up
bindkey "^[[B" down-line-or-beginning-search # Down
