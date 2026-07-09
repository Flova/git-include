mod cli;
mod completions;
mod git;
mod gitrepo;
mod lfs;
mod ops;
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
    // Completions must work outside a repository.
    if let Command::Completions { shell } = cli.command {
        completions::print(shell);
        return Ok(());
    }

    let git = Git::discover(Path::new("."))?;

    match cli.command {
        Command::Add {
            remote,
            subdir,
            branch,
            no_lfs,
        } => {
            let subdir = repo_relative_subdir(&git, &subdir)?;
            ops::add::run(&git, &remote, &subdir, branch.as_deref(), no_lfs)
        }
        Command::Pull {
            subdir,
            all,
            no_lfs,
        } => {
            let subdir = subdir.map(|s| repo_relative_subdir(&git, &s)).transpose()?;
            ops::pull::run(&git, subdir.as_deref(), all, no_lfs)
        }
        Command::Push {
            subdir,
            dry_run,
            squash,
            no_lfs,
        } => {
            let subdir = repo_relative_subdir(&git, &subdir)?;
            ops::push::run(&git, &subdir, dry_run, squash, no_lfs)
        }
        Command::Init {
            subdir,
            remote,
            branch,
        } => {
            let subdir = repo_relative_subdir(&git, &subdir)?;
            ops::init::run(&git, &subdir, &remote, branch.as_deref())
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
            branch,
            no_lfs,
        } => {
            let subdir = repo_relative_subdir(&git, &subdir)?;
            ops::branches::switch(&git, &subdir, &branch, no_lfs)
        }
        Command::Branches { subdir } => {
            let subdir = repo_relative_subdir(&git, &subdir)?;
            ops::branches::list(&git, &subdir)
        }
        Command::List => ops::list::run(&git),
        Command::Remove { subdir } => {
            let subdir = repo_relative_subdir(&git, &subdir)?;
            ops::remove::run(&git, &subdir)
        }
        Command::Completions { .. } => unreachable!("handled above"),
    }
}
