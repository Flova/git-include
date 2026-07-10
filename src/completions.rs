//! Shell completion generation.
//!
//! Two invocation styles must complete:
//!   1. `git-include <TAB>`  — handled by the clap-generated script.
//!   2. `git include <TAB>`  — handled by hooking into git's own completion
//!      (bash calls `_git_include`, zsh dispatches `#compdef git-include`
//!      automatically for user subcommands).

use clap::CommandFactory;
use clap_complete::{Shell, generate};

use crate::cli::{Cli, CompletionShell};

pub fn print(shell: CompletionShell) {
    let mut cmd = Cli::command();
    // The clap-generated scripts must complete the real binary name.
    let shell = match shell {
        CompletionShell::Bash => Shell::Bash,
        CompletionShell::Zsh => Shell::Zsh,
        CompletionShell::Fish => Shell::Fish,
        CompletionShell::Elvish => Shell::Elvish,
        CompletionShell::Powershell => Shell::PowerShell,
    };
    generate(shell, &mut cmd, "git-include", &mut std::io::stdout());

    if matches!(shell, Shell::Bash) {
        print!("{}", BASH_GIT_SUBCOMMAND_SHIM);
    }
    if matches!(shell, Shell::Fish) {
        print!("{}", FISH_GIT_SUBCOMMAND_SHIM);
    }
}

/// git's bash completion dispatches `git include <TAB>` to a function
/// called `_git_include` if it exists. This shim completes subcommands,
/// included directories (queried live from the repository) and branch
/// names for `switch`.
const BASH_GIT_SUBCOMMAND_SHIM: &str = r#"
# --- git subcommand integration: makes `git include <TAB>` work ---------
_git_include() {
    local subcommands="add init migrate pull push status diff switch branches list remove completions help"
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local sub=""
    # COMP_WORDS = (git include <sub> ...)
    if [ "${#COMP_WORDS[@]}" -gt 3 ]; then
        sub="${COMP_WORDS[2]}"
    fi

    __git_include_dirs() {
        git ls-files -- '*.gitrepo' 2>/dev/null | sed 's|/\.gitrepo$||'
    }

    case "$sub" in
        pull|push|status|diff|branches|remove)
            COMPREPLY=($(compgen -W "$(__git_include_dirs)" -- "$cur"))
            ;;
        switch)
            if [ "${COMP_CWORD}" -eq 3 ]; then
                COMPREPLY=($(compgen -W "$(__git_include_dirs)" -- "$cur"))
            else
                local dir="${COMP_WORDS[3]}"
                local remote
                remote=$(git config --file "$dir/.gitrepo" subrepo.remote 2>/dev/null)
                # Only contact remotes with ordinary transports: a cloned
                # repository controls this value, and exotic schemes like
                # ext:: would execute commands via git.
                case "$remote" in
                    https://*|http://*|ssh://*|git://*|git@*)
                        COMPREPLY=($(compgen -W "$(git ls-remote --heads "$remote" 2>/dev/null \
                            | sed 's|.*refs/heads/||')" -- "$cur"))
                        ;;
                esac
            fi
            ;;
        completions)
            COMPREPLY=($(compgen -W "bash zsh fish elvish powershell" -- "$cur"))
            ;;
        init)
            COMPREPLY=($(compgen -d -- "$cur"))
            ;;
        add|list)
            COMPREPLY=()
            ;;
        *)
            COMPREPLY=($(compgen -W "$subcommands" -- "$cur"))
            ;;
    esac
}
"#;

/// fish completes `git include` subcommands through these entries.
const FISH_GIT_SUBCOMMAND_SHIM: &str = r#"
# --- git subcommand integration: makes `git include <TAB>` work ---------
function __fish_git_include_dirs
    git ls-files -- '*.gitrepo' 2>/dev/null | string replace -r '/\.gitrepo$' ''
end
complete -c git -n '__fish_seen_subcommand_from include; and not __fish_seen_subcommand_from add init migrate pull push status diff switch branches list remove completions self-update' \
    -a 'add init migrate pull push status diff switch branches list remove completions self-update'
complete -c git -n '__fish_seen_subcommand_from include; and __fish_seen_subcommand_from pull push status diff switch branches remove' \
    -a '(__fish_git_include_dirs)'
"#;
