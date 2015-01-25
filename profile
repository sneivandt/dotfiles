# Add ~/bin to $PATH
export PATH="$PATH:$HOME/bin"

# Yay Vim!
export EDITOR=vim

# Less
export LESS=-imR

# Dont check mail
unset MAILCHECK

# Start X Windows
[[ -z $DISPLAY && $XDG_VTNR -eq 1 && -x $(which i3 2>/dev/null) ]] && exec startx
