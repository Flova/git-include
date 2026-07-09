//! `git include init` (alias `export`): turn an existing, ordinary
//! directory of the host repository into an included repository.
//!
//! The whole host history is walked and every commit that changed the
//! directory is rebuilt as a commit of a brand-new standalone history —
//! same author, same message, but containing only the directory's content.
//! The marker then points at the tip of that history, so a subsequent
//! `git include push` publishes it (creating the branch on an empty
//! remote), and all normal include operations work from there on.

use anyhow::{Context, Result, bail};

use crate::git::Git;
use crate::gitrepo::{GitRepoFile, MARKER_FILE, validate_subdir};
use crate::ops::commit_message;
use crate::util::{pin_ref, short};

pub fn run(
    git: &Git,
    subdir: &str,
    remote: &str,
    branch: Option<&str>,
    message: Option<&str>,
) -> Result<()> {
    validate_subdir(subdir)?;
    git.require_clean_worktree("init an included repository")?;
    let head = git.head()?;

    if git.toplevel.join(subdir).join(MARKER_FILE).exists() {
        bail!("'{subdir}' is already an included repository (it has a {MARKER_FILE} file)");
    }
    let Some(current) = git.tree_at("HEAD", subdir) else {
        bail!(
            "'{subdir}' has no tracked files in HEAD; commit the directory first \
             (or use `git include add` to vendor an existing upstream)"
        );
    };

    let branch = match branch {
        Some(b) => b.to_string(),
        None => match git.remote_default_branch(remote) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("Remote has no default branch yet; using 'main'.");
                "main".to_string()
            }
        },
    };

    // Rebuild the directory's history: every commit that changed its
    // content becomes a commit of the new standalone history.
    eprintln!("Extracting the history of '{subdir}' ...");
    let empty = git.empty_tree()?;
    let mut tip: Option<String> = None;
    let mut tip_tree = empty.clone();
    let mut count = 0usize;
    for commit in git.walk_all(&head)? {
        let cur = match git.tree_at(&commit, subdir) {
            Some(t) => git.tree_without_entry(&t, MARKER_FILE)?,
            None => empty.clone(),
        };
        if cur == tip_tree {
            continue;
        }
        tip = Some(git.replay_commit(&commit, &cur, tip.as_deref())?);
        tip_tree = cur;
        count += 1;
    }
    let tip = tip.with_context(|| format!("no history found for '{subdir}'"))?;

    // Write the marker and commit it (a marker-only change, so it is a
    // natural no-op for future pushes).
    let meta = GitRepoFile::new(remote, &branch, &tip, Some(&head));
    let stripped = git.tree_without_entry(&current, MARKER_FILE)?;
    let subtree = git.tree_with_blob(&stripped, MARKER_FILE, meta.serialize().as_bytes())?;
    let root = git.root_tree_with_subtree(subdir, Some(&subtree))?;
    git.apply_tree_prefix(&root, subdir)?;
    git.commit_on_head(&commit_message(git, message, "init", subdir, &meta), &root)?;
    // Pin the extracted history so it survives `git gc` until pushed.
    git.set_ref(&pin_ref(subdir), &tip)?;

    println!(
        "Turned '{subdir}' into an included repository: extracted {count} commit(s) \
         of history (head {}).",
        short(&tip)
    );
    println!("Publish it with: git include push {subdir}");
    Ok(())
}
