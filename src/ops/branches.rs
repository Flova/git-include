use anyhow::Result;

use crate::git::Git;
use crate::ops::Include;
use crate::ops::pull::{PullOptions, sync};
use crate::util::short;

/// `git include branches <dir>`: list upstream branches and tags, marking
/// the revision currently tracked.
pub fn list(git: &Git, subdir: &str) -> Result<()> {
    let inc = Include::load(git, subdir)?;
    let refs = git.remote_refs(&inc.meta.remote)?;
    if refs.branches.is_empty() && refs.tags.is_empty() {
        println!("No branches or tags found on {}.", inc.meta.remote);
        return Ok(());
    }
    for (sha, name) in &refs.branches {
        let marker = if *name == inc.meta.branch { "*" } else { " " };
        println!("{marker} {name} ({})", short(sha));
    }
    if !refs.tags.is_empty() {
        println!("\nTags:");
        for (sha, name) in &refs.tags {
            let marker = if *name == inc.meta.branch { "*" } else { " " };
            println!("{marker} {name} ({})", short(sha));
        }
    }
    println!("\n(* = tracked; switch with `git include switch {subdir} <branch|tag|commit>`)");
    Ok(())
}

/// `git include switch <dir> <rev>`: start tracking another upstream
/// branch — or pin to a tag or commit. Local changes relative to the old
/// revision are carried over via a three-way merge (conflicts are
/// surfaced like a pull).
pub fn switch(git: &Git, subdir: &str, rev: &str, opts: &PullOptions<'_>) -> Result<()> {
    let inc = Include::load(git, subdir)?;
    if rev == inc.meta.branch && !opts.force {
        println!("'{subdir}' already tracks '{rev}'.");
        return Ok(());
    }
    sync(inc, Some(rev), None, "switch", opts)
}
