[[ $- != *i* ]] && return

unset MAILCHECK

if [ "$TERM" == "xterm" ]
then
  export TERM=xterm-256color
fi

export PATH="$HOME/bin:$PATH"
export EDITOR=vim

PS1="\[\e[0;37m\]\u@\h\[\e[0m\]:\[\e[0;33m\]\w\[\e[0m\]\n\$ "

alias sudo="sudo "
alias ls="ls --color"
alias la="ls -a --color"
alias ll="ls -lh --color"
alias sl="ls"
alias ..="cd .."
alias ...="cd ../.."
alias cl="clear"
alias vi="vim"
