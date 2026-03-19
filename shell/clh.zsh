# clh — zsh integration
# Source this file in your .zshrc:
#   eval "$(clh setup)"

autoload -Uz add-zsh-hook

# Record every command to clh-server in the background (non-blocking)
_clh_add_history() {
  local last_cmd
  last_cmd=$(fc -ln -1)
  # Skip empty commands and the clh command itself
  [[ -z "$last_cmd" ]] && return
  [[ "$last_cmd" == clh* ]] && return
  clh add --hostname="${HOST:-$(hostname)}" --pwd="$PWD" --command="$last_cmd" &!
}
add-zsh-hook precmd _clh_add_history

# Ctrl+S: fuzzy search filtered to the current directory
_clh_search_pwd_widget() {
  local selected
  selected=$(clh search --pwd="$PWD" 2>/dev/tty)
  local ret=$?
  if [[ $ret -eq 0 && -n "$selected" ]]; then
    LBUFFER="$selected"
  fi
  zle reset-prompt
}
zle -N _clh_search_pwd_widget
bindkey '^S' _clh_search_pwd_widget

# Ctrl+T: fuzzy search across all history
_clh_search_all_widget() {
  local selected
  selected=$(clh search 2>/dev/tty)
  local ret=$?
  if [[ $ret -eq 0 && -n "$selected" ]]; then
    LBUFFER="$selected"
  fi
  zle reset-prompt
}
zle -N _clh_search_all_widget
bindkey '^T' _clh_search_all_widget
