mod cli;
mod completions;
mod git;
mod gitrepo;
mod lfs;
mod ops;
mod template;
mod util;

use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command};
use git::Git;
use util::repo_relative_subdir;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    // Completions and self-update must work outside a repository.
    if let Command::Completions { shell } = cli.command {
        completions::print(shell);
        return Ok(());
    }
    if let Command::SelfUpdate { version, dry_run } = &cli.command {
        return ops::selfupdate::run(version.as_deref(), *dry_run);
    }

    let git = Git::discover(Path::new("."))?;

    match cli.command {
        Command::Add {
            remote,
            subdir,
            branch,
            tag,
            commit,
            message,
            no_lfs,
        } => {
            let subdir = repo_relative_subdir(&git, &subdir)?;
            let rev = branch
                .as_deref()
                .map(|b| (b, git::RevKind::Branch))
                .or(tag.as_deref().map(|t| (t, git::RevKind::Tag)))
                .or(commit.as_deref().map(|c| (c, git::RevKind::Commit)));
            let opts = ops::add::AddOptions {
                rev,
                message: message.as_deref(),
                no_lfs,
            };
            ops::add::run(&git, &remote, &subdir, &opts)
        }
        Command::Pull {
            subdir,
            all,
            force,
            message,
            no_lfs,
        } => {
            let subdir = subdir.map(|s| repo_relative_subdir(&git, &s)).transpose()?;
            let opts = ops::pull::PullOptions {
                force,
                message: message.as_deref(),
                no_lfs,
            };
            ops::pull::run(&git, subdir.as_deref(), all, &opts)
        }
        Command::Push {
            subdir,
            dry_run,
            branch,
            squash,
            message,
            no_lfs,
        } => {
            let subdir = repo_relative_subdir(&git, &subdir)?;
            let opts = ops::push::PushOptions {
                dry_run,
                squash,
                to_branch: branch.as_deref(),
                message: message.as_deref(),
                no_lfs,
            };
            ops::push::run(&git, &subdir, &opts)
        }
        Command::Init {
            subdir,
            remote,
            branch,
            message,
        } => {
            let subdir = repo_relative_subdir(&git, &subdir)?;
            ops::init::run(
                &git,
                &subdir,
                &remote,
                branch.as_deref(),
                message.as_deref(),
            )
        }
        Command::Status { subdir, fetch } => {
            let subdir = subdir.map(|s| repo_relative_subdir(&git, &s)).transpose()?;
            ops::status::run(&git, subdir.as_deref(), fetch)
        }
        Command::Diff {
            subdir,
            upstream,
            fetch,
            stat,
        } => {
            let subdir = repo_relative_subdir(&git, &subdir)?;
            ops::diff::run(&git, &subdir, upstream, fetch, stat)
        }
        Command::Switch {
            subdir,
            rev,
            force,
            message,
            no_lfs,
        } => {
            let subdir = repo_relative_subdir(&git, &subdir)?;
            let opts = ops::pull::PullOptions {
                force,
                message: message.as_deref(),
                no_lfs,
            };
            ops::branches::switch(&git, &subdir, &rev, &opts)
        }
        Command::Branches { subdir } => {
            let subdir = repo_relative_subdir(&git, &subdir)?;
            ops::branches::list(&git, &subdir)
        }
        Command::Remote {
            subdir,
            url,
            message,
        } => {
            let subdir = repo_relative_subdir(&git, &subdir)?;
            ops::remote::run(&git, &subdir, url.as_deref(), message.as_deref())
        }
        Command::List => ops::list::run(&git),
        Command::Remove { subdir, message } => {
            let subdir = repo_relative_subdir(&git, &subdir)?;
            ops::remove::run(&git, &subdir, message.as_deref())
        }
        Command::Completions { .. } | Command::SelfUpdate { .. } => {
            unreachable!("handled above")
        }
    }
}
