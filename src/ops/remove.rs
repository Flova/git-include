use anyhow::Result;

use crate::git::Git;
use crate::ops::{Include, commit_message};

/// `git include remove <dir>`: delete the included directory (files stay in
/// history; upstream is untouched).
pub fn run(git: &Git, subdir: &str, message: Option<&str>) -> Result<()> {
    let inc = Include::load(git, subdir)?;
    git.require_clean_worktree(&format!("remove '{subdir}'"))?;

    let root = git.root_tree_with_subtree(subdir, None)?;
    git.apply_tree_prefix(&root, subdir)?;
    git.commit_on_head(
        &commit_message(git, message, "remove", subdir, &inc.meta),
        &root,
    )?;
    git.delete_ref(&inc.pin_ref());

    println!(
        "Removed included repository '{subdir}' (upstream {} untouched).",
        inc.meta.remote
    );
    Ok(())
}
