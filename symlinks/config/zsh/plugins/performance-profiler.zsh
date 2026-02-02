#!/usr/bin/env zsh
# Performance profiling helper for zsh startup
# To enable profiling, set ZSH_PROFILE=1 before sourcing zshrc
#   Example: ZSH_PROFILE=1 zsh -ic exit

if [[ -n "$ZSH_PROFILE" ]]; then
  zmodload zsh/zprof
fi

# Convenient alias to profile shell startup
alias zsh-profile='ZSH_PROFILE=1 zsh -ic "zprof | head -20"'
alias zsh-startup-time='for i in {1..10}; do time zsh -ic exit; done'
