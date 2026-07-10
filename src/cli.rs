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
        #[arg(short, long, conflicts_with_all = ["tag", "commit"])]
        branch: Option<String>,
        /// Pin to an upstream tag instead of tracking a branch
        #[arg(short, long, conflicts_with = "commit")]
        tag: Option<String>,
        /// Pin to an exact upstream commit id
        #[arg(long)]
        commit: Option<String>,
        /// Commit message template for the sync commit ({{ variable }}
        /// substitution; see the README for available variables)
        #[arg(short, long)]
        message: Option<String>,
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
        /// Pull from this remote instead of the tracked one (e.g. a fork);
        /// the marker is retargeted to it
        #[arg(short, long, conflicts_with = "all")]
        remote: Option<String>,
        /// Discard local changes to the directory and take upstream
        /// verbatim (alias: --discard)
        #[arg(long, alias = "discard")]
        force: bool,
        /// Commit message template for the sync commit
        #[arg(short, long)]
        message: Option<String>,
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
        /// Push to this (possibly new) branch instead of the tracked one
        #[arg(short, long)]
        branch: Option<String>,
        /// Push to this remote instead of the tracked one (e.g. a fork)
        #[arg(short, long)]
        remote: Option<String>,
        /// Keep the marker tracking its current remote/branch instead of
        /// retargeting it to where the push went (temporary-fork PR flow;
        /// requires --branch or --remote)
        #[arg(long)]
        keep: bool,
        /// Push all local changes as a single squashed commit
        #[arg(long)]
        squash: bool,
        /// Commit message template for the local bookkeeping commit
        #[arg(short, long)]
        message: Option<String>,
        /// Skip Git LFS object upload
        #[arg(long)]
        no_lfs: bool,
    },
    /// Turn an existing directory into a new included repository,
    /// extracting its full history from your commits
    #[command(alias = "export")]
    Init {
        /// Tracked directory to turn into an included repository
        subdir: PathBuf,
        /// URL of the (possibly still empty) repository that will host it
        #[arg(short, long)]
        remote: String,
        /// Branch to publish to (default: the remote's default branch,
        /// or 'main' for an empty remote)
        #[arg(short, long)]
        branch: Option<String>,
        /// Commit message template for the sync commit
        #[arg(short, long)]
        message: Option<String>,
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
    /// Switch an included directory to another branch, tag, or commit
    Switch {
        /// Included directory
        subdir: PathBuf,
        /// Branch to track, or tag/commit to pin to
        rev: String,
        /// Resolve the revision on (and retarget the marker to) this
        /// remote instead of the tracked one
        #[arg(short, long)]
        remote: Option<String>,
        /// Discard local changes instead of carrying them over
        /// (alias: --discard; default is to keep them via a merge)
        #[arg(long, alias = "discard")]
        force: bool,
        /// Commit message template for the sync commit
        #[arg(short, long)]
        message: Option<String>,
        /// Skip Git LFS object download
        #[arg(long)]
        no_lfs: bool,
    },
    /// List the upstream branches and tags of an included repository
    Branches {
        /// Included directory
        subdir: PathBuf,
    },
    /// Show or change the upstream remote of an included directory
    Remote {
        /// Included directory
        subdir: PathBuf,
        /// New remote URL (omit to print the current one)
        url: Option<String>,
        /// Commit message template for the sync commit
        #[arg(short, long)]
        message: Option<String>,
    },
    /// List all included repositories (including nested ones)
    List,
    /// Remove an included directory (upstream is untouched)
    Remove {
        /// Included directory
        subdir: PathBuf,
        /// Commit message template for the removal commit
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Generate shell tab-completion scripts
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: CompletionShell,
    },
    /// Update git-include itself to the latest release
    SelfUpdate {
        /// Install a specific version instead of the latest (e.g. v0.2.0)
        #[arg(long)]
        version: Option<String>,
        /// Only check what would be installed
        #[arg(short = 'n', long)]
        dry_run: bool,
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
