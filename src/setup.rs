use std::path::Path;

/// Supported shell types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shell {
    Zsh,
    Bash,
    Fish,
}

impl Shell {
    /// Detect the current shell from the `$SHELL` environment variable.
    pub fn detect() -> Option<Self> {
        let shell = std::env::var("SHELL").ok()?;
        let name = Path::new(&shell).file_name()?.to_str()?;
        match name {
            "zsh" => Some(Self::Zsh),
            "bash" => Some(Self::Bash),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }

    /// Parse from a string (e.g. from `--shell` CLI arg).
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "zsh" => Some(Self::Zsh),
            "bash" => Some(Self::Bash),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }
}

const ZSH_SCRIPT: &str = r#"# clh — zsh integration
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
  # Skip commands prefixed with a space (intentionally hidden from history)
  [[ "$last_cmd" == " "* ]] && return
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
"#;

const BASH_SCRIPT: &str = r#"# clh — bash integration
# Add to your ~/.bashrc:
#   eval "$(clh setup)"

# Ctrl+S is traditionally XOFF — disable it so we can bind it
stty -ixon 2>/dev/null

# Record every command to clh-server in the background (non-blocking)
_clh_add_history() {
  local last_cmd
  last_cmd=$(HISTTIMEFORMAT='' history 1 | sed 's/^ *[0-9]* *//')
  [[ -z "$last_cmd" ]] && return
  [[ "$last_cmd" == clh* ]] && return
  # Skip commands prefixed with a space (intentionally hidden from history)
  [[ "$last_cmd" == " "* ]] && return
  clh add --hostname="${HOSTNAME:-$(hostname)}" --pwd="$PWD" --command="$last_cmd" &
}
PROMPT_COMMAND="_clh_add_history${PROMPT_COMMAND:+;$PROMPT_COMMAND}"

# Ctrl+S: fuzzy search filtered to the current directory
_clh_search_pwd() {
  local selected
  selected=$(clh search --pwd="$PWD" 2>/dev/tty)
  local ret=$?
  if [[ $ret -eq 0 && -n "$selected" ]]; then
    READLINE_LINE="$selected"
    READLINE_POINT=${#selected}
  fi
}
bind -x '"\C-s": _clh_search_pwd'

# Ctrl+T: fuzzy search across all history
_clh_search_all() {
  local selected
  selected=$(clh search 2>/dev/tty)
  local ret=$?
  if [[ $ret -eq 0 && -n "$selected" ]]; then
    READLINE_LINE="$selected"
    READLINE_POINT=${#selected}
  fi
}
bind -x '"\C-t": _clh_search_all'
"#;

const FISH_SCRIPT: &str = r#"# clh — fish integration
# Add to your ~/.config/fish/config.fish:
#   clh setup | source

# Record every command to clh-server in the background (non-blocking)
function _clh_add_history --on-event fish_postexec
    set last_cmd $argv[1]
    test -z "$last_cmd"; and return
    string match -q 'clh*' -- $last_cmd; and return
    # Skip commands prefixed with a space (intentionally hidden from history)
    string match -q ' *' -- $last_cmd; and return
    clh add --hostname=(hostname) --pwd=(pwd) --command="$last_cmd" &
end

# Ctrl+S: fuzzy search filtered to the current directory
function _clh_search_pwd
    set selected (clh search --pwd=(pwd) 2>/dev/tty)
    if test -n "$selected"
        commandline $selected
    end
    commandline -f repaint
end

# Ctrl+T: fuzzy search across all history
function _clh_search_all
    set selected (clh search 2>/dev/tty)
    if test -n "$selected"
        commandline $selected
    end
    commandline -f repaint
end

bind \cs _clh_search_pwd
bind \ct _clh_search_all
"#;

/// Print the integration script for the given shell.
/// Falls back to auto-detection from `$SHELL` if `shell` is `None`.
pub fn print_setup(shell: Option<&str>) -> anyhow::Result<()> {
    let resolved = match shell {
        Some(s) => Shell::from_str(s)
            .ok_or_else(|| anyhow::anyhow!("Unsupported shell: {s}. Supported: zsh, bash, fish"))?,
        None => Shell::detect().ok_or_else(|| {
            anyhow::anyhow!(
                "Could not detect shell from $SHELL. Use --shell <zsh|bash|fish> explicitly."
            )
        })?,
    };

    let script = match resolved {
        Shell::Zsh => ZSH_SCRIPT,
        Shell::Bash => BASH_SCRIPT,
        Shell::Fish => FISH_SCRIPT,
    };
    print!("{script}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_from_str_known_shells() {
        assert_eq!(Shell::from_str("zsh"), Some(Shell::Zsh));
        assert_eq!(Shell::from_str("bash"), Some(Shell::Bash));
        assert_eq!(Shell::from_str("fish"), Some(Shell::Fish));
    }

    #[test]
    fn shell_from_str_unknown_returns_none() {
        assert_eq!(Shell::from_str("nushell"), None);
        assert_eq!(Shell::from_str(""), None);
    }

    #[test]
    fn print_setup_unknown_shell_returns_error() {
        let result = print_setup(Some("nushell"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nushell"));
    }

    #[test]
    fn print_setup_explicit_shell_succeeds() {
        // Just verify no error is returned; output goes to stdout
        assert!(print_setup(Some("zsh")).is_ok());
        assert!(print_setup(Some("bash")).is_ok());
        assert!(print_setup(Some("fish")).is_ok());
    }
}
