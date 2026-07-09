use anyhow::{Context, Result, bail};

use crate::git::Git;
use crate::gitrepo::MARKER_FILE;
use crate::lfs;
use crate::ops::{Include, commit_message};
use crate::util::short;

/// The rebuilt upstream history a push would create: for every host commit
/// since the marker's `parent` that changed the included directory, the
/// subdirectory tree that replaying (cherry-picking) it onto the upstream
/// tip produces.
pub struct ReplayPlan {
    /// (host commit, resulting subdirectory tree), oldest first.
    pub steps: Vec<(String, String)>,
    /// Set when a commit could not be replayed cleanly: (commit, files).
    /// Steps before the conflict are still valid; the remaining changes
    /// have to be pushed as one combined commit.
    pub conflict: Option<(String, Vec<String>)>,
}

/// Walk every host commit in `parent..HEAD` and cherry-pick its
/// subdirectory changes onto a growing upstream tip (starting at
/// `start_commit`, normally the marker's recorded upstream commit).
///
/// Each commit contributes the *diff* of the included directory between
/// its first parent and itself, three-way merged onto the tip. This makes
/// sync commits (pulls, marker updates) natural no-ops — their changes are
/// already upstream — so individual local commits survive across pulls.
pub fn plan_replay(inc: &Include<'_>, start_commit: &str) -> Result<ReplayPlan> {
    let git = inc.git;
    let mut plan = ReplayPlan {
        steps: Vec::new(),
        conflict: None,
    };
    let Some(parent) = inc.meta.parent.as_deref() else {
        return Ok(plan);
    };
    let empty = git.empty_tree()?;
    let mut tip_tree = git
        .rev_parse(&format!("{start_commit}^{{tree}}"))
        .context("replay base commit has no tree")?;

    for commit in git.walk_range(parent, &git.head()?)? {
        let Some(cur) = git.tree_at(&commit, &inc.subdir) else {
            // The directory was deleted in this commit; deleting the whole
            // upstream project is never what a push should do.
            bail!(
                "commit {} removes '{}' entirely; refusing to push a deletion upstream",
                short(&commit),
                inc.subdir
            );
        };
        let cur = git.tree_without_entry(&cur, MARKER_FILE)?;
        let prev = match git.tree_at(&format!("{commit}^"), &inc.subdir) {
            Some(t) => git.tree_without_entry(&t, MARKER_FILE)?,
            None => empty.clone(),
        };
        if cur == prev {
            continue; // did not change the included directory
        }
        let merged = if prev == tip_tree {
            cur.clone() // applies verbatim
        } else {
            let (merged, conflicts) = git.merge_trees_3way(&prev, &tip_tree, &cur)?;
            if !conflicts.is_empty() {
                plan.conflict = Some((commit, conflicts));
                return Ok(plan);
            }
            merged
        };
        if merged == tip_tree {
            continue; // no-op on top of the tip (e.g. pull / sync commits)
        }
        tip_tree = merged.clone();
        plan.steps.push((commit, merged));
    }
    Ok(plan)
}

pub fn run(git: &Git, subdir: &str, dry_run: bool, squash: bool, no_lfs: bool) -> Result<()> {
    let inc = Include::load(git, subdir)?;
    git.require_clean_worktree(&format!("push '{subdir}'"))?;

    eprintln!("Fetching {} ({}) ...", inc.meta.remote, inc.meta.branch);
    let upstream = fetch_upstream_head(&inc)?;

    if let Some(upstream) = &upstream {
        inc.ensure_base_commit()?;
        if *upstream != inc.meta.commit {
            if git.is_descendant_of(upstream, &inc.meta.commit) {
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
                short(upstream),
            );
        }
    } else {
        // Branch does not exist upstream yet (e.g. right after `init`):
        // the recorded history itself is what gets published.
        eprintln!(
            "Branch '{}' does not exist on {} yet; it will be created.",
            inc.meta.branch, inc.meta.remote
        );
        if !git.has_commit(&inc.meta.commit) {
            bail!(
                "the commit recorded in {subdir}/{MARKER_FILE} does not exist locally; \
                 cannot publish '{subdir}'"
            );
        }
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

    let plan = plan_replay(&inc, &inc.meta.commit)?;
    let local = inc.local_tree_stripped()?;

    // Build the new upstream history.
    let mut tip = inc.meta.commit.clone();
    let mut replayed = 0usize;
    if squash {
        let tip_tree = git
            .rev_parse(&format!("{tip}^{{tree}}"))
            .context("recorded commit has no tree")?;
        if local != tip_tree {
            let mut msg = format!("git include push {subdir} (squashed)\n\nSquashed commits:\n");
            for (commit, _) in &plan.steps {
                msg.push_str(&format!(
                    "  {} {}\n",
                    short(commit),
                    git.commit_summary(commit)
                ));
            }
            if dry_run {
                println!("would push: 1 squashed commit");
            }
            tip = git.new_commit(&local, &tip, msg.trim_end())?;
            replayed = 1;
        }
    } else {
        for (commit, tree) in &plan.steps {
            if dry_run {
                println!(
                    "would push: {} {}",
                    short(commit),
                    git.commit_summary(commit)
                );
            }
            // Replay commits are cheap unreferenced objects (gc-able), so a
            // dry run builds them too — identical logic to a real push.
            tip = git.replay_commit(commit, tree, Some(&tip))?;
            replayed += 1;
        }
        // Whatever could not be expressed as individual replays (a commit
        // that conflicts when cherry-picked alone — e.g. its resolution
        // only exists in a later merge — or residual drift) goes into one
        // final commit with the exact current content.
        let tip_tree = git
            .rev_parse(&format!("{tip}^{{tree}}"))
            .context("replay tip has no tree")?;
        if local != tip_tree {
            let msg = match &plan.conflict {
                Some((commit, _)) => {
                    eprintln!(
                        "note: commit {} does not replay cleanly on its own; \
                         combining the remaining changes into one commit.",
                        short(commit)
                    );
                    format!(
                        "git include push {subdir}\n\nCombined local changes \
                         (starting at {} \"{}\", which conflicts when replayed alone).",
                        short(commit),
                        git.commit_summary(commit)
                    )
                }
                None => format!("git include push {subdir}\n\nReconcile local content."),
            };
            if dry_run {
                println!("would push: 1 combined commit for the remaining changes");
            }
            tip = git.new_commit(&local, &tip, &msg)?;
            replayed += 1;
        }
    }

    if replayed == 0 && upstream.is_some() {
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

    // Record the new upstream position in the marker file (one bookkeeping
    // commit; a no-op for future replays since the content is unchanged).
    let mut meta = inc.meta.clone();
    meta.commit = tip.clone();
    meta.parent = Some(git.head()?);
    meta.cmdver = env!("CARGO_PKG_VERSION").to_string();
    let subtree = git.tree_with_blob(&local, MARKER_FILE, meta.serialize().as_bytes())?;
    inc.commit_subtree(&subtree, &commit_message("push", subdir, &meta))?;

    if upstream.is_none() {
        println!(
            "Published '{subdir}' to {} as new branch '{}' (head {}).",
            inc.meta.remote,
            inc.meta.branch,
            short(&tip)
        );
    } else {
        println!(
            "Pushed {replayed} commit(s) from '{subdir}' to {} ({}); upstream is now {}.",
            inc.meta.remote,
            inc.meta.branch,
            short(&tip)
        );
    }
    Ok(())
}

/// Fetch the upstream branch head; Ok(None) when the branch simply does
/// not exist on the remote yet (a reachable remote is still required).
fn fetch_upstream_head(inc: &Include<'_>) -> Result<Option<String>> {
    match inc
        .git
        .fetch_branch(&inc.meta.remote, &inc.meta.branch, &inc.pin_ref())
    {
        Ok(sha) => Ok(Some(sha)),
        Err(err) => match inc.git.remote_branches(&inc.meta.remote) {
            Ok(branches) if !branches.iter().any(|(_, n)| n == &inc.meta.branch) => Ok(None),
            _ => Err(err),
        },
    }
}
