# If not running interactivly, dont't do anything
[[ $- != *i* ]] && return

# Dont check mail
unset MAILCHECK

# Prompt
PS1="\u@\h \[\e[0;33m\]\w\n\[\e[0m\]\$ "

# Aliases
source ~/.bash_aliases
