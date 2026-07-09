use anyhow::{Context, Result, bail};

use crate::git::Git;
use crate::gitrepo::MARKER_FILE;
use crate::lfs;
use crate::ops::{Include, commit_message};
use crate::util::short;

pub fn run(git: &Git, subdir: &str, dry_run: bool, no_lfs: bool) -> Result<()> {
    let inc = Include::load(git, subdir)?;
    git.require_clean_worktree(&format!("push '{subdir}'"))?;

    eprintln!("Fetching {} ({}) ...", inc.meta.remote, inc.meta.branch);
    let upstream = git.fetch_branch(&inc.meta.remote, &inc.meta.branch, &inc.pin_ref())?;
    inc.ensure_base_commit()?;

    if upstream != inc.meta.commit {
        if git.is_descendant_of(&upstream, &inc.meta.commit) {
            bail!(
                "upstream branch '{}' has new commits since the last sync of '{subdir}'.\n\
                 Run `git include pull {subdir}` first, then push again.",
                inc.meta.branch
            );
        }
        bail!(
            "upstream branch '{}' has diverged from the commit recorded in \
             {subdir}/{MARKER_FILE}\n(recorded {}, upstream is now {}). \
             Run `git include pull {subdir}` to reconcile.",
            inc.meta.branch,
            short(&inc.meta.commit),
            short(&upstream),
        );
    }

    let parent = inc.meta.parent.clone().with_context(|| {
        format!("{subdir}/{MARKER_FILE} has no 'parent' entry; cannot determine local commits")
    })?;
    if !git.has_commit(&parent) {
        bail!(
            "the parent commit {} recorded in {subdir}/{MARKER_FILE} does not exist locally\n\
             (host history may have been rewritten). Run `git include pull {subdir}` to re-sync.",
            short(&parent)
        );
    }

    // Local commits since the last sync, oldest first, replayed onto
    // upstream as commits rooted at the subdirectory. Commits that do not
    // change the subdirectory content (including marker-only bookkeeping
    // commits) are skipped.
    let head = git.head()?;
    let mut tip = inc.meta.commit.clone();
    let mut replayed = 0usize;
    for commit in git.walk_range(&parent, &head)? {
        let Some(tree) = git.tree_at(&commit, subdir) else {
            // The directory was deleted in this commit; deleting the whole
            // upstream project is never what a push should do.
            bail!(
                "commit {} removes '{subdir}' entirely; refusing to push a deletion upstream",
                short(&commit)
            );
        };
        let stripped = git.tree_without_entry(&tree, MARKER_FILE)?;
        let tip_tree = git
            .rev_parse(&format!("{tip}^{{tree}}"))
            .context("replay tip has no tree")?;
        if stripped == tip_tree {
            continue;
        }
        if dry_run {
            println!(
                "would push: {} {}",
                short(&commit),
                git.commit_summary(&commit)
            );
        }
        // Replay commits are cheap unreferenced objects (gc-able), so a dry
        // run builds them too — the logic stays identical to a real push.
        tip = git.replay_commit(&commit, &stripped, &tip)?;
        replayed += 1;
    }

    if replayed == 0 {
        println!("'{subdir}' has no local changes to push.");
        return Ok(());
    }
    if dry_run {
        println!(
            "dry run: {replayed} commit(s) would be pushed to {} ({}).",
            inc.meta.remote, inc.meta.branch
        );
        return Ok(());
    }

    lfs::push_objects(git, &inc.meta.remote, &tip, subdir, no_lfs);
    git.push_commit(&inc.meta.remote, &tip, &inc.meta.branch)?;
    git.set_ref(&inc.pin_ref(), &tip)?;

    // Record the new upstream position in the marker file (one
    // bookkeeping commit; skipped by future replays since the content
    // does not change).
    let mut meta = inc.meta.clone();
    meta.commit = tip.clone();
    meta.parent = Some(git.head()?);
    meta.cmdver = env!("CARGO_PKG_VERSION").to_string();
    let current = git
        .tree_at("HEAD", subdir)
        .context("include directory missing from HEAD")?;
    let stripped = git.tree_without_entry(&current, MARKER_FILE)?;
    let subtree = git.tree_with_blob(&stripped, MARKER_FILE, meta.serialize().as_bytes())?;
    inc.commit_subtree(&subtree, &commit_message("push", subdir, &meta))?;

    println!(
        "Pushed {replayed} commit(s) from '{subdir}' to {} ({}); upstream is now {}.",
        inc.meta.remote,
        inc.meta.branch,
        short(&tip)
    );
    Ok(())
}
