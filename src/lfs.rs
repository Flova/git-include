//! Best-effort Git LFS integration.
//!
//! When an included repository uses LFS, plain object fetches only bring in
//! the small pointer files. git-include detects this (via `filter=lfs` in
//! any `.gitattributes` of the fetched tree) and transparently fetches the
//! real content from the upstream LFS store into the host repository, then
//! materializes it in the working tree.
//!
//! This is the single place where git-include shells out to git: LFS is
//! itself an external `git lfs` CLI extension with no library API.
//! Everything here is best-effort — a missing `git-lfs` degrades to a
//! clear warning instead of a hard failure.

use std::process::Command;

use git2::{Oid, TreeWalkMode, TreeWalkResult};

use crate::git::Git;

pub fn lfs_installed() -> bool {
    Command::new("git")
        .args(["lfs", "version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_lfs(git: &Git, args: &[&str]) -> Result<(), String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(&git.toplevel)
        .arg("lfs")
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Does the tree of `commit` contain any `.gitattributes` configuring LFS?
pub fn tree_uses_lfs(git: &Git, commit: &str) -> bool {
    let Some(tree) = Oid::from_str(commit)
        .ok()
        .and_then(|oid| git.repo.find_commit(oid).ok())
        .and_then(|c| c.tree().ok())
    else {
        return false;
    };
    let mut found = false;
    let _ = tree.walk(TreeWalkMode::PreOrder, |_, entry| {
        if entry.name().map(|n| n == ".gitattributes").unwrap_or(false)
            && let Ok(obj) = entry.to_object(&git.repo)
            && let Some(blob) = obj.as_blob()
            && String::from_utf8_lossy(blob.content()).contains("filter=lfs")
        {
            found = true;
            return TreeWalkResult::Abort;
        }
        TreeWalkResult::Ok
    });
    found
}

/// After new upstream content landed in `subdir`, pull the LFS objects for
/// `commit` from `remote` and replace pointer files with real content.
pub fn fetch_and_checkout(git: &Git, remote: &str, commit: &str, subdir: &str, no_lfs: bool) {
    if no_lfs || !tree_uses_lfs(git, commit) {
        return;
    }
    if !lfs_installed() {
        eprintln!(
            "warning: '{subdir}' uses Git LFS but git-lfs is not installed.\n\
             warning: LFS files were checked out as pointer files. Install git-lfs and run:\n\
             warning:   git lfs fetch {remote} {commit} && git lfs checkout -- {subdir}"
        );
        return;
    }
    eprintln!("Fetching Git LFS objects for '{subdir}' ...");
    match run_lfs(git, &["fetch", remote, commit]) {
        Ok(()) => {
            if let Err(err) = run_lfs(git, &["checkout", "--", subdir]) {
                eprintln!("warning: git lfs checkout failed for '{subdir}': {err}");
            }
        }
        Err(err) => {
            eprintln!("warning: could not fetch LFS objects from {remote}: {err}");
        }
    }
}

/// Before pushing `commit` to `remote`, upload any LFS objects it
/// references so the server never sees dangling pointers.
pub fn push_objects(git: &Git, remote: &str, commit: &str, subdir: &str, no_lfs: bool) {
    if no_lfs || !tree_uses_lfs(git, commit) {
        return;
    }
    if !lfs_installed() {
        eprintln!(
            "warning: '{subdir}' uses Git LFS but git-lfs is not installed; \
             LFS content referenced by your commits was NOT uploaded to {remote}."
        );
        return;
    }
    eprintln!("Pushing Git LFS objects for '{subdir}' ...");
    if let Err(err) = run_lfs(git, &["push", remote, commit]) {
        eprintln!("warning: git lfs push to {remote} failed: {err}");
    }
}
