#!/usr/bin/env zsh

# Movement keys
bindkey '\e[3~'     delete-char        # Delete
if [[ "$TERM" == (rxvt*) ]]
then
  bindkey '\e[7~'   beginning-of-line  # Home
  bindkey '\e[8~'   end-of-line        # End
  bindkey '\e[7^'   beginning-of-line  # Ctrl + Home
  bindkey '\e[8^'   end-of-line        # Ctrl + End
  bindkey '^H'      backward-kill-word # Ctrl + Backspace
  bindkey '\e[3^'   kill-word          # Ctrl + Delete
  bindkey '\e[1;5D' backward-word      # Ctrl + Left
  bindkey '\e[1;5C' forward-word       # Ctrl + Right
elif [[ "$TERM" == (screen*) ]]
then
  bindkey '\e[1~'   beginning-of-line  # Home
  bindkey '\e[4~'   end-of-line        # End
  bindkey '\e[1;5H' beginning-of-line  # Ctrl + Home
  bindkey '\e[1;5F' end-of-line        # Ctrl + End
                                       # Ctrl + Backspace
  bindkey '\e[3;5~' kill-word          # Ctrl + Delete
  bindkey '\e[1;5D' backward-word      # Ctrl + Left
  bindkey '\e[1;5C' forward-word       # Ctrl + Right
elif [[ "$TERM" == (xterm*) ]]
then
  bindkey '\e[H'    beginning-of-line  # Home
  bindkey '\e[F'    end-of-line        # End
  bindkey '\e[1;5H' beginning-of-line  # Ctrl + Home
  bindkey '\e[1;5F' end-of-line        # Ctrl + End
  bindkey '^H'      backward-kill-word # Ctrl + Backspace
  bindkey '\e[3;5~' kill-word          # Ctrl + Delete
  bindkey '\e[1;5D' backward-word      # Ctrl + Left
  bindkey '\e[1;5C' forward-word       # Ctrl + Right
fi

# Magic space history expansion
bindkey ' ' magic-space

# Shift+Tab Reverse menu complete
bindkey '^[[Z' reverse-menu-complete

# Ctrl+r Search history
bindkey '^R' history-incremental-search-backward

# Ctrl+t Toggle "sudo " at the start of line
sudo-command-line()
{
  [[ -z $BUFFER ]] && zle up-history
  if [[ $BUFFER == sudo\ * ]]; then
    LBUFFER="${LBUFFER#sudo }"
  else
    LBUFFER="sudo $LBUFFER"
  fi
}
zle -N sudo-command-line
bindkey '^T' sudo-command-line

# Search based on what you typed so far
autoload -U up-line-or-beginning-search
autoload -U down-line-or-beginning-search
zle -N up-line-or-beginning-search
zle -N down-line-or-beginning-search
bindkey "^[[A" up-line-or-beginning-search # Up
bindkey "^[[B" down-line-or-beginning-search # Down
