use anyhow::{Context, Result, bail};

use crate::git::{Git, RevKind};
use crate::gitrepo::MARKER_FILE;
use crate::lfs;
use crate::ops::{Include, commit_message, find_all_includes};
use crate::util::short;

pub struct PullOptions<'a> {
    pub force: bool,
    /// Pull from this remote instead of the tracked one; the marker is
    /// retargeted to it (pull always updates the marker).
    pub remote: Option<&'a str>,
    pub message: Option<&'a str>,
    pub no_lfs: bool,
}

pub fn run(git: &Git, subdir: Option<&str>, all: bool, opts: &PullOptions<'_>) -> Result<()> {
    let targets: Vec<String> = if all {
        find_all_includes(git)?
    } else if let Some(s) = subdir {
        vec![s.to_string()]
    } else {
        // Convenience: a single include needs no argument.
        let includes = find_all_includes(git)?;
        match includes.len() {
            0 => bail!("no included repositories found; use `git include add` first"),
            1 => includes,
            _ => bail!(
                "multiple included repositories exist; pass a directory or --all:\n  {}",
                includes.join("\n  ")
            ),
        }
    };

    let action = if opts.force { "pull --force" } else { "pull" };
    for subdir in &targets {
        let inc = Include::load(git, subdir)?;
        sync(inc, None, None, action, opts)?;
    }
    Ok(())
}

/// Shared sync engine used by `pull` (same ref) and `switch` (another
/// branch/tag/commit). Fetches upstream, three-way merges it with local
/// changes and commits the result — or leaves conflict markers for the
/// user to resolve. With `force`, local changes to the directory are
/// discarded and upstream content is taken verbatim.
pub fn sync(
    inc: Include<'_>,
    new_rev: Option<&str>,
    expect: Option<RevKind>,
    action: &str,
    opts: &PullOptions<'_>,
) -> Result<()> {
    let git = inc.git;
    if !opts.force {
        git.require_clean_worktree(&format!("{action} '{}'", inc.subdir))?;
    }

    let rev = new_rev.unwrap_or(&inc.meta.branch).to_string();
    let remote = opts.remote.unwrap_or(&inc.meta.remote).to_string();
    eprintln!("Fetching {remote} ({rev}) ...");
    let (upstream, kind) = git.fetch_rev(&remote, &rev, expect, &inc.pin_ref())?;

    let upstream_tree = git
        .rev_parse(&format!("{upstream}^{{tree}}"))
        .context("fetched commit has no tree")?;
    let same_target =
        upstream == inc.meta.commit && rev == inc.meta.branch && remote == inc.meta.remote;
    if same_target && !opts.force {
        match kind {
            RevKind::Branch => println!("'{}' is already up to date with {rev}.", inc.subdir),
            _ => println!(
                "'{}' is up to date (pinned to {} '{rev}').",
                inc.subdir,
                kind.label()
            ),
        }
        return Ok(());
    }

    let (merged_stripped, conflicts) = if opts.force {
        // Discard local state: upstream verbatim.
        (upstream_tree.clone(), Vec::new())
    } else {
        inc.ensure_base_commit()?;
        let base_tree = inc
            .base_tree()
            .context("base commit exists but has no tree")?;
        let local_stripped = inc.local_tree_stripped()?;
        if local_stripped == base_tree || local_stripped == upstream_tree {
            // No local changes since the last sync (or content already
            // matches upstream): take upstream verbatim, no merge needed.
            (upstream_tree.clone(), Vec::new())
        } else {
            git.merge_trees_3way(&base_tree, &local_stripped, &upstream_tree)?
        }
    };

    // Attach the updated marker file to the merged tree. Note that
    // `parent` is deliberately NOT advanced by a regular pull: it marks
    // the last host commit whose changes are already upstream, so local
    // commits made before this pull can still be pushed individually
    // later. A force pull discards local changes, so there it DOES
    // advance (nothing local is left to push).
    let mut meta = inc.meta.clone();
    meta.remote = remote.clone();
    meta.branch = rev.clone();
    meta.commit = upstream.clone();
    if opts.force || meta.parent.is_none() {
        meta.parent = Some(git.head()?);
    }
    meta.cmdver = env!("CARGO_PKG_VERSION").to_string();
    meta.ref_kind_hint = Some(kind);
    let subtree = git.tree_with_blob(&merged_stripped, MARKER_FILE, meta.serialize().as_bytes())?;

    if !conflicts.is_empty() {
        // Materialize the conflicted result (with the marker update staged)
        // but do not commit; the user resolves and commits.
        let root = git.root_tree_with_subtree(&inc.subdir, Some(&subtree))?;
        git.apply_tree_prefix(&root, &inc.subdir)?;

        eprintln!(
            "\nCONFLICT: could not automatically merge upstream changes into '{}'.",
            inc.subdir
        );
        eprintln!("Files with conflict markers:");
        for f in &conflicts {
            eprintln!("  {}/{f}", inc.subdir);
        }
        eprintln!(
            "\nResolve the conflicts, then finish with:\n  \
             git add {0}\n  git commit\n\n\
             (the staged {0}/{MARKER_FILE} update is already correct — keep it)\n\
             To discard your local changes instead: git include pull {0} --force",
            inc.subdir
        );
        bail!("merge conflicts in '{}'", inc.subdir);
    }

    let before = git.head()?;
    let after = inc.commit_subtree(
        &subtree,
        &commit_message(git, opts.message, action, &inc.subdir, &meta),
    )?;
    lfs::fetch_and_checkout(git, &meta.remote, &upstream, &inc.subdir, opts.no_lfs);

    if before == after {
        println!("'{}' already matches {} '{rev}'.", inc.subdir, kind.label());
        return Ok(());
    }
    match (new_rev.is_some(), kind) {
        (true, RevKind::Branch) => println!(
            "Switched '{}' to branch {rev} (commit {}).",
            inc.subdir,
            short(&upstream)
        ),
        (true, kind) => println!(
            "Pinned '{}' to {} '{rev}' (commit {}).",
            inc.subdir,
            kind.label(),
            short(&upstream)
        ),
        (false, _) => println!(
            "Pulled '{}': now at {rev} commit {}.",
            inc.subdir,
            short(&upstream)
        ),
    }
    if remote != inc.meta.remote {
        println!("'{}' now tracks {remote}.", inc.subdir);
    }
    Ok(())
}
