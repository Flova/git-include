use anyhow::{Context, Result, bail};

use crate::git::Git;
use crate::gitrepo::MARKER_FILE;
use crate::lfs;
use crate::ops::{Include, commit_message, find_all_includes};
use crate::util::short;

pub fn run(git: &Git, subdir: Option<&str>, all: bool, no_lfs: bool) -> Result<()> {
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

    for subdir in &targets {
        let inc = Include::load(git, subdir)?;
        sync(inc, None, "pull", no_lfs)?;
    }
    Ok(())
}

/// Shared sync engine used by `pull` (same branch) and `switch` (another
/// branch). Fetches upstream, three-way merges it with local changes and
/// commits the result — or leaves conflict markers for the user to resolve.
pub fn sync(inc: Include<'_>, new_branch: Option<&str>, action: &str, no_lfs: bool) -> Result<()> {
    let git = inc.git;
    git.require_clean_worktree(&format!("{action} '{}'", inc.subdir))?;

    let branch = new_branch.unwrap_or(&inc.meta.branch).to_string();
    eprintln!("Fetching {} ({branch}) ...", inc.meta.remote);
    let upstream = git.fetch_branch(&inc.meta.remote, &branch, &inc.pin_ref())?;

    if upstream == inc.meta.commit && branch == inc.meta.branch {
        println!("'{}' is already up to date with {branch}.", inc.subdir);
        return Ok(());
    }

    inc.ensure_base_commit()?;
    let base_tree = inc
        .base_tree()
        .context("base commit exists but has no tree")?;
    let local_stripped = inc.local_tree_stripped()?;
    let upstream_tree = git
        .rev_parse(&format!("{upstream}^{{tree}}"))
        .context("fetched commit has no tree")?;

    let (merged_stripped, conflicts) =
        if local_stripped == base_tree || local_stripped == upstream_tree {
            // No local changes since the last sync (or content already
            // matches upstream): take upstream verbatim, no merge needed.
            (upstream_tree, Vec::new())
        } else {
            let (merged, conflicts) =
                git.merge_trees_3way(&base_tree, &local_stripped, &upstream_tree)?;
            (merged, conflicts)
        };

    // Attach the updated marker file to the merged tree. Note that
    // `parent` is deliberately NOT advanced by a pull: it marks the last
    // host commit whose changes are already upstream, so local commits
    // made before this pull can still be pushed individually later.
    let mut meta = inc.meta.clone();
    meta.branch = branch.clone();
    meta.commit = upstream.clone();
    if meta.parent.is_none() {
        meta.parent = Some(git.head()?);
    }
    meta.cmdver = env!("CARGO_PKG_VERSION").to_string();
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
             (the staged {0}/{MARKER_FILE} update is already correct — keep it)",
            inc.subdir
        );
        bail!("merge conflicts in '{}'", inc.subdir);
    }

    inc.commit_subtree(&subtree, &commit_message(action, &inc.subdir, &meta))?;
    lfs::fetch_and_checkout(git, &meta.remote, &upstream, &inc.subdir, no_lfs);

    if new_branch.is_some() {
        println!(
            "Switched '{}' to branch {branch} (commit {}).",
            inc.subdir,
            short(&upstream)
        );
    } else {
        println!(
            "Pulled '{}': now at {branch} commit {}.",
            inc.subdir,
            short(&upstream)
        );
    }
    Ok(())
}
