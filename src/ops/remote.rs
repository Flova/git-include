//! `git include remote <dir> [<url>]`: show or change the upstream remote
//! of an include (e.g. after a repository moved, or to point at a fork).

use anyhow::{Result, bail};

use crate::git::{Git, looks_like_oid};
use crate::gitrepo::MARKER_FILE;
use crate::ops::{Include, commit_message};
use crate::util::short;

pub fn run(git: &Git, subdir: &str, url: Option<&str>, message: Option<&str>) -> Result<()> {
    let inc = Include::load(git, subdir)?;

    let Some(url) = url else {
        println!("{}", inc.meta.remote);
        return Ok(());
    };
    if url == inc.meta.remote {
        println!("'{subdir}' already uses remote {url}.");
        return Ok(());
    }
    git.require_clean_worktree(&format!("change the remote of '{subdir}'"))?;

    // The tracked revision must exist on the new remote, otherwise every
    // later pull/push would fail in confusing ways.
    let refs = git.remote_refs(url)?;
    let rev = &inc.meta.branch;
    let known = refs.branches.iter().any(|(_, n)| n == rev)
        || refs.tags.iter().any(|(_, n)| n == rev)
        || looks_like_oid(rev);
    if !known {
        bail!(
            "the tracked revision '{rev}' does not exist on {url}.\n\
             Switch to one of its branches first: git include switch {subdir} <branch>"
        );
    }

    // Refresh the pin from the new remote (also proves it serves the rev).
    let (sha, kind) = git.fetch_rev(url, rev, None, &inc.pin_ref())?;

    let mut meta = inc.meta.clone();
    meta.remote = url.to_string();
    meta.cmdver = env!("CARGO_PKG_VERSION").to_string();
    meta.ref_kind_hint = Some(kind);
    let current = inc.local_tree_stripped()?;
    let subtree = git.tree_with_blob(&current, MARKER_FILE, meta.serialize().as_bytes())?;
    inc.commit_subtree(
        &subtree,
        &commit_message(git, message, "remote", subdir, &meta),
    )?;

    println!(
        "'{subdir}' now uses remote {url} ({} '{rev}' is at {}).",
        kind.label(),
        short(&sha)
    );
    if sha != meta.commit {
        println!(
            "note: '{rev}' on the new remote differs from the last-synced commit {}; \
             run `git include status {subdir}` / `pull` to reconcile.",
            short(&meta.commit)
        );
    }
    Ok(())
}
