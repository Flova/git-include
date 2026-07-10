use std::collections::HashMap;

use anyhow::{Context, Result, bail};

use crate::git::{Git, looks_like_oid};
use crate::gitrepo::{GitRepoFile, MARKER_FILE};
use crate::lfs;
use crate::ops::{Include, commit_message};
use crate::util::short;

/// The rebuilt upstream history a push would create: a 1:1 image of the
/// host commits since the marker's `parent`, restricted to the included
/// directory. Branching and merging are preserved exactly as they happened
/// on the host; commits that do not touch the directory are pruned.
pub struct ReplayPlan {
    /// (host commit, its rebuilt upstream commit), oldest first. Pruned
    /// host commits do not appear.
    pub steps: Vec<(String, String)>,
    /// The rebuilt image of host HEAD — what push publishes. Equals the
    /// recorded upstream commit when there is nothing to push.
    pub tip: String,
}

/// Map every host commit in `parent..HEAD` to its image in the rebuilt
/// upstream history: same message and author, the included directory's
/// tree taken verbatim (marker stripped), and the host parents translated
/// to their own images — so the host's branch/merge topology carries over
/// unchanged.
///
/// Pruning keeps the result free of noise: commits that leave the
/// directory as their parent's image already has it simply reuse that
/// image (this also collapses merges whose other leg never touched the
/// directory), and history from before the include existed maps to
/// nothing at all. Sync commits (pull, switch, push bookkeeping) map to
/// the upstream commit they took — a pull that merged local work becomes
/// a real merge with upstream, which is also what keeps the rebuilt
/// history a fast-forward of the upstream branch.
pub fn plan_replay(inc: &Include<'_>, start_commit: &str) -> Result<ReplayPlan> {
    let git = inc.git;
    let mut plan = ReplayPlan {
        steps: Vec::new(),
        tip: start_commit.to_string(),
    };
    let Some(parent) = inc.meta.parent.as_deref() else {
        return Ok(plan);
    };
    let base = replay_base(inc, parent, start_commit);

    // The upstream image of every host commit mapped so far. None: pruned
    // with nothing upstream to stand in (history predating the include).
    let mut images: HashMap<String, Option<String>> = HashMap::new();
    images.insert(parent.to_string(), Some(base.clone()));

    let head = git.head()?;
    for commit in git.walk_range(parent, &head)? {
        let host_parents = git.commit_parents(&commit)?;
        let mut parents: Vec<String> = Vec::new();
        for p in &host_parents {
            let image = match images.get(p) {
                Some(image) => image.clone(),
                // Not in `parent..HEAD`, so its content is already
                // upstream: at its own recorded sync point if it has one,
                // at the replay base otherwise.
                None => hidden_image(inc, p, &base),
            };
            if let Some(image) = image
                && !parents.contains(&image)
            {
                parents.push(image);
            }
        }
        drop_redundant_parents(git, &mut parents);

        let Some(subdir_tree) = git.tree_at(&commit, &inc.subdir) else {
            // The directory existed in a parent and this commit deletes
            // it; deleting the whole upstream project is never what a
            // push should do.
            if host_parents
                .iter()
                .any(|p| git.tree_at(p, &inc.subdir).is_some())
            {
                bail!(
                    "commit {} removes '{}' entirely; refusing to push a deletion upstream",
                    short(&commit),
                    inc.subdir
                );
            }
            // History from before the include existed (e.g. a side branch
            // that forked earlier and was merged in later): nothing of it
            // can concern the include.
            images.insert(commit, None);
            continue;
        };
        let tree = git.tree_without_entry(&subdir_tree, MARKER_FILE)?;

        // A commit that moved the marker to another upstream commit is a
        // sync, and its directory content came from upstream, not from
        // local work. Taken verbatim it IS that upstream commit; merged
        // with local changes it becomes a real merge with upstream.
        if let Some(upstream) = sync_target(inc, &commit, host_parents.first().map(String::as_str))
        {
            if git.rev_parse(&format!("{upstream}^{{tree}}")).as_deref() == Some(tree.as_str()) {
                images.insert(commit, Some(upstream));
                continue;
            }
            if !parents.contains(&upstream) {
                parents.push(upstream);
                drop_redundant_parents(git, &mut parents);
            }
        }

        // Unchanged against the sole surviving parent: the commit did not
        // touch the directory (or is a merge that collapsed once the legs
        // it joined were pruned) — it needs no image of its own.
        if parents.len() == 1
            && git
                .rev_parse(&format!("{}^{{tree}}", parents[0]))
                .as_deref()
                == Some(tree.as_str())
        {
            images.insert(commit, parents.pop());
            continue;
        }

        let rebuilt = git.replay_commit(&commit, &tree, &parents)?;
        images.insert(commit.clone(), Some(rebuilt.clone()));
        plan.steps.push((commit, rebuilt));
    }

    if let Some(Some(tip)) = images.get(&head) {
        plan.tip = tip.clone();
    }
    Ok(plan)
}

/// The upstream commit the rebuilt history grows from: the one the marker
/// recorded at the range boundary `parent` — everything up to `parent` is
/// already upstream at exactly that commit. (Not the *currently* recorded
/// commit: local commits made before a later pull were based on the older
/// state, and their images must say so. The pull's own image then merges
/// the two lines.) Falls back to the current commit when the marker at
/// `parent` is unreadable or its commit cannot be obtained.
fn replay_base(inc: &Include<'_>, parent: &str, start_commit: &str) -> String {
    let git = inc.git;
    let Some(meta) = marker_meta_at(inc, parent) else {
        return start_commit.to_string();
    };
    if !git.has_commit(&meta.commit) {
        git.try_fetch_commit(&inc.meta.remote, &meta.commit);
    }
    if git.has_commit(&meta.commit) {
        meta.commit
    } else {
        start_commit.to_string()
    }
}

/// Upstream image of a commit outside `parent..HEAD` (an ancestor of
/// `parent`, so already upstream): its own recorded sync point when its
/// marker names one that exists locally, the replay base otherwise. None
/// for commits from before the include existed.
fn hidden_image(inc: &Include<'_>, commit: &str, base: &str) -> Option<String> {
    let git = inc.git;
    git.tree_at(commit, &inc.subdir)?;
    match marker_meta_at(inc, commit) {
        Some(meta) if git.has_commit(&meta.commit) => Some(meta.commit),
        _ => Some(base.to_string()),
    }
}

/// The upstream commit a sync commit took: Some when `commit` moved the
/// marker's recorded commit (pull, switch, push bookkeeping) and that
/// commit is obtainable locally.
fn sync_target(inc: &Include<'_>, commit: &str, first_parent: Option<&str>) -> Option<String> {
    let git = inc.git;
    let now = marker_meta_at(inc, commit)?;
    if first_parent
        .and_then(|p| marker_meta_at(inc, p))
        .is_some_and(|before| before.commit == now.commit)
    {
        return None; // the marker did not move
    }
    if !git.has_commit(&now.commit) {
        git.try_fetch_commit(&inc.meta.remote, &now.commit);
    }
    git.has_commit(&now.commit).then_some(now.commit)
}

/// Parse the marker file as recorded in `rev`, if present and readable.
fn marker_meta_at(inc: &Include<'_>, rev: &str) -> Option<GitRepoFile> {
    let git = inc.git;
    let blob = git.rev_parse(&format!("{rev}:{}/{MARKER_FILE}", inc.subdir))?;
    let blob = git.repo.find_blob(git2::Oid::from_str(&blob).ok()?).ok()?;
    GitRepoFile::parse(&String::from_utf8_lossy(blob.content())).ok()
}

/// Drop parents that are ancestors of another parent: merges whose other
/// leg was pruned (or was already upstream) degrade to ordinary commits
/// instead of degenerate merges.
fn drop_redundant_parents(git: &Git, parents: &mut Vec<String>) {
    if parents.len() < 2 {
        return;
    }
    let all = parents.clone();
    parents.retain(|p| {
        !all.iter()
            .any(|other| other != p && git.is_descendant_of(other, p))
    });
}

pub struct PushOptions<'a> {
    pub dry_run: bool,
    pub squash: bool,
    /// Push to this (possibly new) branch instead of the tracked one.
    pub to_branch: Option<&'a str>,
    /// Push to this remote instead of the tracked one (e.g. a fork).
    pub to_remote: Option<&'a str>,
    /// Keep the marker tracking its current remote/branch instead of
    /// retargeting it to where the push went (temporary-fork PR flow).
    pub keep: bool,
    pub message: Option<&'a str>,
    pub no_lfs: bool,
}

pub fn run(git: &Git, subdir: &str, opts: &PushOptions<'_>) -> Result<()> {
    let (dry_run, squash, no_lfs) = (opts.dry_run, opts.squash, opts.no_lfs);
    let inc = Include::load(git, subdir)?;
    git.require_clean_worktree(&format!("push '{subdir}'"))?;

    let target_remote = opts.to_remote.unwrap_or(&inc.meta.remote).to_string();
    let target_branch = opts.to_branch.unwrap_or(&inc.meta.branch).to_string();
    let elsewhere = target_remote != inc.meta.remote || target_branch != inc.meta.branch;
    if opts.keep && !elsewhere {
        bail!("--keep only makes sense together with --branch/--remote");
    }

    eprintln!("Fetching {} ({}) ...", inc.meta.remote, inc.meta.branch);
    let upstream = if elsewhere {
        // Pushing somewhere other than the tracked revision — a new branch
        // and/or another remote (e.g. a fork for an upstream pull request).
        // The target branch, if it already exists, must sit exactly at the
        // recorded base so we never clobber unrelated work.
        let refs = git.remote_refs(&target_remote)?;
        if refs.tags.iter().any(|(_, n)| *n == target_branch) {
            bail!(
                "'{target_branch}' is a tag on {target_remote}; pass --branch to pick a \
                 branch to push to"
            );
        }
        if looks_like_oid(&target_branch) && !refs.branches.iter().any(|(_, n)| *n == target_branch)
        {
            bail!(
                "'{}' is pinned to commit {}; pass --branch to pick a branch to push to",
                inc.subdir,
                short(&target_branch)
            );
        }
        let target = refs
            .branches
            .iter()
            .find(|(_, n)| *n == target_branch)
            .map(|(sha, _)| sha.clone());
        if let Some(sha) = &target
            && *sha != inc.meta.commit
        {
            bail!(
                "branch '{target_branch}' already exists on {target_remote} at {} (not at \
                 the recorded base {}).\nPick a new branch name, or push to it from a \
                 matching state.",
                short(sha),
                short(&inc.meta.commit),
            );
        }
        if target.is_none() {
            eprintln!(
                "Branch '{target_branch}' does not exist on {target_remote} yet; \
                 it will be created."
            );
        }
        // Make sure the base commit's objects are available locally.
        let _ = git.fetch_rev(&inc.meta.remote, &inc.meta.branch, None, &inc.pin_ref());
        inc.ensure_base_commit()?;
        target
    } else {
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
        upstream
    };

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
        // The plan already built the rebuilt history (cheap unreferenced
        // objects, gc-able — a dry run builds them too, identical to a
        // real push); this only publishes its head.
        if dry_run {
            for (commit, _) in &plan.steps {
                println!(
                    "would push: {} {}",
                    short(commit),
                    git.commit_summary(commit)
                );
            }
        }
        tip = plan.tip.clone();
        replayed = if tip == inc.meta.commit {
            0
        } else {
            plan.steps.len()
        };
        // Safety net: by construction the rebuilt head carries exactly the
        // current directory content; reconcile any drift in one commit.
        let tip_tree = git
            .rev_parse(&format!("{tip}^{{tree}}"))
            .context("replay tip has no tree")?;
        if local != tip_tree {
            if dry_run {
                println!("would push: 1 combined commit for the remaining changes");
            }
            let msg = format!("git include push {subdir}\n\nReconcile local content.");
            tip = git.new_commit(&local, &tip, &msg)?;
            replayed += 1;
        }
    }

    if replayed == 0 && upstream.is_some() {
        if elsewhere {
            println!(
                "'{subdir}' has no local changes to push to {target_remote} ({target_branch})."
            );
            println!(
                "To only retarget the include, use `git include pull --remote` or \
                 `git include switch`."
            );
        } else {
            println!("'{subdir}' has no local changes to push.");
        }
        return Ok(());
    }
    if dry_run {
        println!(
            "dry run: {replayed} commit(s) would be pushed to {target_remote} ({target_branch})."
        );
        return Ok(());
    }

    lfs::push_objects(git, &target_remote, &tip, subdir, no_lfs);
    git.push_commit(&target_remote, &tip, &target_branch)?;

    if opts.keep {
        // Temporary-fork flow: the include keeps tracking its original
        // remote/branch. The local commits stay "to push" until they reach
        // the tracked revision (e.g. once the pull request is merged).
        println!(
            "Pushed {replayed} commit(s) from '{subdir}' to {target_remote} ({target_branch})."
        );
        println!(
            "'{subdir}' still tracks {} ({}); pull once the changes land there.",
            inc.meta.remote, inc.meta.branch
        );
        return Ok(());
    }

    git.set_ref(&inc.pin_ref(), &tip)?;

    // Record the new upstream position in the marker file (one bookkeeping
    // commit; a no-op for future replays since the content is unchanged).
    // A push to another remote/branch retargets the include there.
    let mut meta = inc.meta.clone();
    meta.remote = target_remote.clone();
    meta.branch = target_branch.clone();
    meta.commit = tip.clone();
    meta.parent = Some(git.head()?);
    meta.cmdver = env!("CARGO_PKG_VERSION").to_string();
    meta.ref_kind_hint = Some(crate::git::RevKind::Branch);
    let subtree = git.tree_with_blob(&local, MARKER_FILE, meta.serialize().as_bytes())?;
    inc.commit_subtree(
        &subtree,
        &commit_message(git, opts.message, "push", subdir, &meta),
    )?;

    if upstream.is_none() {
        println!(
            "Published '{subdir}' to {target_remote} as new branch '{target_branch}' (head {}).",
            short(&tip)
        );
    } else {
        println!(
            "Pushed {replayed} commit(s) from '{subdir}' to {target_remote} \
             ({target_branch}); upstream is now {}.",
            short(&tip)
        );
    }
    if elsewhere {
        println!("'{subdir}' now tracks {target_remote} ({target_branch}).");
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
