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

/// The marker's remote value can come from a repository someone else
/// authored. libgit2 only speaks real transports, but `git lfs` goes
/// through git itself, where option-like values or exotic schemes such as
/// `ext::` can execute commands — so the CLI shell-out is restricted to
/// ordinary remotes.
fn remote_safe_for_cli(remote: &str) -> bool {
    !remote.starts_with('-') && !remote.contains("::")
}

/// git-lfs only speaks real URLs: handed a plain local path (a common
/// remote form for filesystem repositories), its batch API fails with
/// "missing protocol". Rewrite such remotes as file:// URLs, which
/// git-lfs serves through its standalone file adapter.
fn cli_remote(remote: &str) -> String {
    let path = std::path::Path::new(remote);
    if remote.contains("://") || !path.is_dir() {
        return remote.to_string();
    }
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut s = abs.to_string_lossy().replace('\\', "/");
    // Windows canonicalize yields \\?\C:\... — file URLs want /C:/...
    s = s.trim_start_matches("//?/").to_string();
    if !s.starts_with('/') {
        s.insert(0, '/');
    }
    format!("file://{s}")
}

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
    if !remote_safe_for_cli(remote) {
        eprintln!(
            "warning: refusing to pass unusual remote '{remote}' to the git-lfs CLI; \
             LFS files stay as pointer files."
        );
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
    match run_lfs(git, &["fetch", &cli_remote(remote), commit]) {
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
    if !remote_safe_for_cli(remote) {
        eprintln!(
            "warning: refusing to pass unusual remote '{remote}' to the git-lfs CLI; \
             LFS objects were NOT uploaded."
        );
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
    if let Err(err) = run_lfs(git, &["push", &cli_remote(remote), commit]) {
        eprintln!("warning: git lfs push to {remote} failed: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::cli_remote;

    #[test]
    fn urls_and_nonexistent_paths_pass_through() {
        assert_eq!(cli_remote("https://x/y.git"), "https://x/y.git");
        assert_eq!(cli_remote("git@host:me/repo.git"), "git@host:me/repo.git");
        assert_eq!(cli_remote("/does/not/exist"), "/does/not/exist");
    }

    #[test]
    fn local_directories_become_file_urls() {
        let dir = std::env::temp_dir();
        let url = cli_remote(dir.to_str().unwrap());
        assert!(url.starts_with("file:///"), "got: {url}");
    }
}
