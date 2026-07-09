use anyhow::{Context, Result, bail};

use crate::git::{Git, looks_like_oid};
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
        if is_verbatim_sync_commit(inc, &commit, &cur) {
            // A sync commit that took some upstream state verbatim (force
            // pull, add, clean switch): its diff is real but its content
            // never represents local work — skip it explicitly.
            continue;
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

/// Did `commit` update the marker file AND leave the directory content at
/// exactly the upstream tree its marker records? That combination uniquely
/// identifies sync commits that took upstream verbatim (force pull, add,
/// clean switch) — content that must never be replayed as local work.
fn is_verbatim_sync_commit(inc: &Include<'_>, commit: &str, stripped_tree: &str) -> bool {
    let git = inc.git;
    let marker_path = format!("{}/{MARKER_FILE}", inc.subdir);
    let marker_now = git.rev_parse(&format!("{commit}:{marker_path}"));
    let marker_before = git.rev_parse(&format!("{commit}^:{marker_path}"));
    if marker_now.is_none() || marker_now == marker_before {
        return false; // not a sync commit
    }
    let Some(recorded) = marker_now
        .and_then(|blob| git.repo.find_blob(git2::Oid::from_str(&blob).ok()?).ok())
        .and_then(|blob| {
            crate::gitrepo::GitRepoFile::parse(&String::from_utf8_lossy(blob.content())).ok()
        })
        .map(|meta| meta.commit)
    else {
        return false;
    };
    git.rev_parse(&format!("{recorded}^{{tree}}")).as_deref() == Some(stripped_tree)
}

pub struct PushOptions<'a> {
    pub dry_run: bool,
    pub squash: bool,
    pub message: Option<&'a str>,
    pub no_lfs: bool,
}

pub fn run(git: &Git, subdir: &str, opts: &PushOptions<'_>) -> Result<()> {
    let (dry_run, squash, no_lfs) = (opts.dry_run, opts.squash, opts.no_lfs);
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
    inc.commit_subtree(
        &subtree,
        &commit_message(git, opts.message, "push", subdir, &meta),
    )?;

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

/// Fetch the upstream branch head. Ok(None) when the branch simply does
/// not exist on the remote yet (a reachable remote is still required),
/// and a clear error when the include is pinned to a tag or commit —
/// there is nothing sensible to push to in that case.
fn fetch_upstream_head(inc: &Include<'_>) -> Result<Option<String>> {
    let git = inc.git;
    let rev = &inc.meta.branch;
    let refs = git.remote_refs(&inc.meta.remote)?;
    if refs.branches.iter().any(|(_, n)| n == rev) {
        let (sha, _) = git.fetch_rev(
            &inc.meta.remote,
            rev,
            Some(crate::git::RevKind::Branch),
            &inc.pin_ref(),
        )?;
        return Ok(Some(sha));
    }
    if refs.tags.iter().any(|(_, n)| n == rev) {
        bail!(
            "'{}' is pinned to tag '{rev}'; pushing to a tag is not possible.\n\
             Track a branch first: git include switch {} <branch>",
            inc.subdir,
            inc.subdir
        );
    }
    if looks_like_oid(rev) {
        bail!(
            "'{}' is pinned to commit {}; there is no branch to push to.\n\
             Track a branch first: git include switch {} <branch>",
            inc.subdir,
            crate::util::short(rev),
            inc.subdir
        );
    }
    // An unknown branch name: it will be created by this push.
    Ok(None)
}
