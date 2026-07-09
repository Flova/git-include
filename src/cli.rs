use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Vendor external git repositories as plain files, with full two-way sync.
///
/// git-include inlines an upstream repository into a subdirectory of your
/// repository, plus a small `.gitrepo` marker file (compatible with
/// git-subrepo). Collaborators just clone and build — no submodule dance.
#[derive(Parser)]
#[command(
    name = "git-include",
    bin_name = "git include",
    version,
    about,
    long_about = None,
    disable_help_subcommand = false
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Include an upstream repository as a subdirectory
    Add {
        /// URL of the upstream repository (or a configured remote name)
        remote: String,
        /// Directory to place the included repository in
        subdir: PathBuf,
        /// Upstream branch to track (default: the remote's default branch)
        #[arg(short, long)]
        branch: Option<String>,
        /// Skip Git LFS object download even if upstream uses LFS
        #[arg(long)]
        no_lfs: bool,
    },
    /// Merge new upstream commits into an included directory
    Pull {
        /// Included directory (optional when only one include exists)
        subdir: Option<PathBuf>,
        /// Pull every included repository
        #[arg(long, conflicts_with = "subdir")]
        all: bool,
        /// Skip Git LFS object download
        #[arg(long)]
        no_lfs: bool,
    },
    /// Send local commits of an included directory to its upstream
    Push {
        /// Included directory
        subdir: PathBuf,
        /// Show what would be pushed without pushing
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// Skip Git LFS object upload
        #[arg(long)]
        no_lfs: bool,
    },
    /// Show sync state of included repositories (local + upstream)
    Status {
        /// Included directory (default: all)
        subdir: Option<PathBuf>,
        /// Contact upstream to refresh its state first
        #[arg(short, long)]
        fetch: bool,
    },
    /// Diff an included directory against its upstream
    Diff {
        /// Included directory
        subdir: PathBuf,
        /// Compare against the latest upstream head instead of the
        /// last-synced commit
        #[arg(short, long)]
        upstream: bool,
        /// Contact upstream to refresh its state first
        #[arg(short, long)]
        fetch: bool,
        /// Show a diffstat instead of the full patch
        #[arg(long)]
        stat: bool,
    },
    /// Switch an included directory to another upstream branch
    Switch {
        /// Included directory
        subdir: PathBuf,
        /// Upstream branch to switch to
        branch: String,
        /// Skip Git LFS object download
        #[arg(long)]
        no_lfs: bool,
    },
    /// List the upstream branches of an included repository
    Branches {
        /// Included directory
        subdir: PathBuf,
    },
    /// List all included repositories (including nested ones)
    List,
    /// Remove an included directory (upstream is untouched)
    Remove {
        /// Included directory
        subdir: PathBuf,
    },
    /// Generate shell tab-completion scripts
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: CompletionShell,
    },
}

#[derive(Copy, Clone, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Elvish,
    Powershell,
}
