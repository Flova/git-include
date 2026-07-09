use anyhow::Result;

use crate::git::Git;
use crate::ops::Include;
use crate::ops::pull::sync;
use crate::util::short;

/// `git include branches <dir>`: list upstream branches, marking the one
/// currently tracked.
pub fn list(git: &Git, subdir: &str) -> Result<()> {
    let inc = Include::load(git, subdir)?;
    let branches = git.remote_branches(&inc.meta.remote)?;
    if branches.is_empty() {
        println!("No branches found on {}.", inc.meta.remote);
        return Ok(());
    }
    for (sha, name) in branches {
        let marker = if name == inc.meta.branch { "*" } else { " " };
        println!("{marker} {name} ({})", short(&sha));
    }
    println!("\n(* = tracked; switch with `git include switch {subdir} <branch>`)");
    Ok(())
}

/// `git include switch <dir> <branch>`: start tracking another upstream
/// branch. Local changes relative to the old branch are carried over via a
/// three-way merge (conflicts are surfaced like a pull).
pub fn switch(git: &Git, subdir: &str, branch: &str, no_lfs: bool) -> Result<()> {
    let inc = Include::load(git, subdir)?;
    if branch == inc.meta.branch {
        println!("'{subdir}' already tracks branch '{branch}'.");
        return Ok(());
    }
    sync(inc, Some(branch), "switch", no_lfs)
}
