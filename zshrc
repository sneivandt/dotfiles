# Colors
autoload -U colors
colors

# History
setopt histignorealldups
setopt incappendhistory
HISTFILE=~/.zhistory
HISTSIZE=4096
SAVEHIST=4096

# Various options
setopt autocd
setopt autopushd
setopt pushdminus
setopt pushdsilent
setopt pushdtohome

# Prompt
source ~/.zsh/prompt

# Completion
source ~/.zsh/completion

# Key bindings
source ~/.zsh/key-bindings

# Aliases
source ~/.aliases
