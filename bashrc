[[ $- != *i* ]] && return

unset MAILCHECK

if [ "$TERM" == "xterm" ]
then
  export TERM=xterm-256color
fi

PS1='\[\e[0;37m\]\u@\h\[\e[0m\]:\[\e[0;33m\]\w\[\e[0m\]\n\$ '

export EDITOR=vim

alias sudo='sudo '
alias sl='ls --color'
alias ls='ls --color'
alias la='ls -a --color'
alias ll='ls -la --color'
alias ..='cd ..'
alias cl='clear'
alias vi='vim'
