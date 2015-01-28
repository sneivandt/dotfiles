# Add ~/bin to $PATH
export PATH="$PATH:$HOME/bin"

# Locale
export LC_ALL=en_US.UTF-8
export LANG=en_US.UTF-8
export LANGUAGE=en_US.UTF-8

# Yay Vim!
export EDITOR=vim

# Less
export LESS=-imR

# Dont check mail
unset MAILCHECK

# Start X Windows
[[ -z $DISPLAY && $XDG_VTNR -eq 1 && -x $(which i3 2>/dev/null) ]] && exec startx